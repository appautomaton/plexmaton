//! Bounded ownership and commit ordering for automatic and requested context compaction.

use plexmaton_agent::{
    BudgetDecision, BudgetPressure, CompactionAttemptFinished, CompactionFailure, CompactionId,
    CompactionOutcome, ModelCall, ModelError, OversizedInput, RequestAttemptId,
};
use plexmaton_provider::{
    CompactionInput, CompactionPreparationError, PreparedCompaction, budget_ledger,
    plan_compaction, validate_compaction_output,
};
use tokio_util::sync::CancellationToken;

use super::{LiveRuntime, RetainedFuture, transition::AfterCommit};
use crate::{RequestedCompactionOutcome, RuntimeError};

mod requested;
mod state;
pub(in crate::runtime) use state::{CompactionOperation, TurnCompactionBudget};
use state::{CompactionPhase, Continuation};

pub(in crate::runtime) use state::{CompactionTrigger, DEFAULT_COMPACTION_TIMEOUT};

impl LiveRuntime {
    fn next_compaction_id(&self) -> CompactionId {
        CompactionId::new(format!(
            "compaction-j{}",
            self.agent.journal().next_sequence().get()
        ))
        .unwrap_or_else(|error| unreachable!("bounded compaction identity is valid: {error}"))
    }

    pub(super) fn cancel_compaction_continuation(&mut self) {
        let Some(operation) = self.compaction.as_mut() else {
            return;
        };
        operation.continuation = Continuation::Cancelled;
    }

    pub(super) fn route_model_call(&mut self, mut call: ModelCall) -> Result<(), RuntimeError> {
        // A queued next turn can be opened while the prior terminal delivery is still committing.
        // Preserve that existing deferral before consulting context or starting another provider.
        if self.active.is_some() {
            return self.authorize_model(call);
        }
        if self.compaction.is_some() {
            return Err(RuntimeError::CompactionAlreadyActive);
        }
        if let Err(reason) = self.resolve_collaboration_call(&mut call) {
            let reaction =
                self.agent
                    .fail_collaboration_request(&call.step_id, &reason, self.clock.now())?;
            return self.begin_transition(reaction, Vec::new(), AfterCommit::None);
        }
        let Some((model, tools)) = self.driver.budget_inputs() else {
            return self.authorize_model(call);
        };
        let ledger = budget_ledger(
            self.agent.journal(),
            self.agent.selected_head(),
            model,
            tools,
        )
        .map_err(RuntimeError::ContextBudget)?;
        let trigger = match (&ledger.decision, call.step_id.index()) {
            (BudgetDecision::Fits, _) => None,
            (
                BudgetDecision::CompactionNeeded {
                    pressure: BudgetPressure::SoftLimit,
                },
                1,
            ) => Some(CompactionTrigger::TurnStartSoft),
            (
                BudgetDecision::CompactionNeeded {
                    pressure: BudgetPressure::HardLimit,
                }
                | BudgetDecision::ImpossibleItem {
                    item: OversizedInput::Atom { .. },
                },
                _,
            ) => Some(CompactionTrigger::HardLimit),
            (
                BudgetDecision::ImpossibleItem {
                    item: OversizedInput::Environment,
                },
                _,
            ) => Some(CompactionTrigger::HardLimit),
            (
                BudgetDecision::CompactionNeeded {
                    pressure: BudgetPressure::SoftLimit,
                },
                _,
            ) => None,
        };
        match trigger {
            Some(trigger) => self.begin_compaction(call, trigger),
            None => self.authorize_model(call),
        }
    }

    pub(super) fn begin_compaction(
        &mut self,
        call: ModelCall,
        trigger: CompactionTrigger,
    ) -> Result<(), RuntimeError> {
        if self.compaction.is_some() {
            return Err(RuntimeError::CompactionAlreadyActive);
        }
        if !self.compaction_budget.has_summary_attempt(&call.step_id) {
            return self.finish_compaction_failure(call, trigger);
        }
        let Some((model, tools)) = self.driver.budget_inputs() else {
            return self.finish_compaction_failure(call, trigger);
        };
        let id = self.next_compaction_id();
        let prepared = match plan_compaction(
            self.agent.journal(),
            self.agent.selected_head(),
            model,
            tools,
            id,
        ) {
            Ok(prepared) => prepared,
            Err(_) => return self.finish_compaction_failure(call, trigger),
        };
        let input = prepared.input().clone();
        self.start_compaction_attempt(
            prepared,
            input,
            Continuation::ModelCall {
                original: call,
                trigger,
            },
        )
    }

    fn start_compaction_attempt(
        &mut self,
        prepared: PreparedCompaction,
        input: CompactionInput,
        continuation: Continuation,
    ) -> Result<(), RuntimeError> {
        // Automatic work spends the turn's summary budget; a request is one attempt, no loop.
        let continuation = match continuation {
            Continuation::ModelCall { original, trigger } => {
                if !self
                    .compaction_budget
                    .take_summary_attempt(&original.step_id)
                {
                    return self.finish_compaction_failure(original, trigger);
                }
                Continuation::ModelCall { original, trigger }
            }
            Continuation::Requested => Continuation::Requested,
            Continuation::Cancelled => return Err(RuntimeError::CompactionContinuationMissing),
        };
        let requested = matches!(continuation, Continuation::Requested);
        let (attempt_id, reaction) = self
            .agent
            .authorize_compaction_attempt(prepared.plan(), self.clock.now())
            .map_err(RuntimeError::CompactionRefused)?;
        self.compaction = Some(CompactionOperation {
            prepared,
            continuation,
            phase: CompactionPhase::Authorizing { attempt_id, input },
            requested,
        });
        if let Err(error) =
            self.begin_transition(reaction, Vec::new(), AfterCommit::StartCompaction)
        {
            self.compaction = None;
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn start_authorized_compaction(&mut self) -> Result<(), RuntimeError> {
        let mut operation = self
            .compaction
            .take()
            .ok_or(RuntimeError::CompactionContinuationMissing)?;
        let phase = std::mem::replace(&mut operation.phase, CompactionPhase::Transitioning);
        let CompactionPhase::Authorizing { attempt_id, input } = phase else {
            self.compaction = Some(operation);
            return Err(RuntimeError::CompactionContinuationMissing);
        };
        if matches!(operation.continuation, Continuation::Cancelled) {
            let finished = cancelled_before_dispatch(attempt_id, input);
            let reaction = self
                .agent
                .finish_compaction_attempt(finished.clone())
                .map_err(RuntimeError::CompactionRefused)?;
            operation.phase = CompactionPhase::Finishing { finished };
            self.compaction = Some(operation);
            return self.begin_transition(reaction, Vec::new(), AfterCommit::FinishCompaction);
        }
        let cancellation = CancellationToken::new();
        let future = self.driver.summarize(
            attempt_id.clone(),
            input,
            operation.prepared.plan().max_summary_bytes(),
            cancellation.child_token(),
        );
        operation.phase = CompactionPhase::Running {
            cancellation,
            future: RetainedFuture::new(future),
            deadline: Box::pin(tokio::time::sleep(self.compaction_timeout)),
            timed_out: false,
        };
        self.compaction = Some(operation);
        Ok(())
    }

    pub(super) fn compaction_deadline_reached(&mut self) {
        let Some(operation) = self.compaction.as_mut() else {
            return;
        };
        if let CompactionPhase::Running {
            cancellation,
            timed_out,
            ..
        } = &mut operation.phase
        {
            *timed_out = true;
            cancellation.cancel();
        }
    }

    pub(super) async fn compaction_ended(
        &mut self,
        result: Result<CompactionAttemptFinished, ()>,
    ) -> Result<(), RuntimeError> {
        let timed_out = self.compaction.as_ref().is_some_and(|operation| {
            matches!(
                operation.phase,
                CompactionPhase::Running {
                    timed_out: true,
                    ..
                }
            )
        });
        let Some(mut finished) = result.ok() else {
            return self.finish_abnormal_compaction().await;
        };
        if timed_out {
            finished = CompactionAttemptFinished::new(
                finished.terminal().clone(),
                finished.input_mode().clone(),
                CompactionOutcome::Failed {
                    kind: CompactionFailure::TimedOut,
                    output: finished.outcome().output().cloned(),
                },
            )
            .unwrap_or_else(|error| unreachable!("timeout preserves a valid terminal: {error}"));
        } else if let CompactionOutcome::Complete { output } = finished.outcome()
            && let Some((model, tools)) = self.driver.budget_inputs()
            && let Some(prepared) = self
                .compaction
                .as_ref()
                .map(|operation| &operation.prepared)
            && let Err(error) = validate_compaction_output(prepared, model, tools, output)
        {
            let kind = match error {
                CompactionPreparationError::OutputTooLarge
                | CompactionPreparationError::UnfittableReplacement => {
                    CompactionFailure::OutputTooLarge
                }
                CompactionPreparationError::NoUsefulReduction
                | CompactionPreparationError::ReplacementMakesNoProgress => {
                    CompactionFailure::NoProgress
                }
                CompactionPreparationError::Source(_)
                | CompactionPreparationError::Budget(_)
                | CompactionPreparationError::Plan(_)
                | CompactionPreparationError::UnfittableEnvironment
                | CompactionPreparationError::OversizedRequiredUser
                | CompactionPreparationError::NoFittingInput => CompactionFailure::Malformed,
            };
            finished = CompactionAttemptFinished::new(
                finished.terminal().clone(),
                finished.input_mode().clone(),
                CompactionOutcome::Failed {
                    kind,
                    output: Some(output.clone()),
                },
            )
            .unwrap_or_else(|error| unreachable!("replacement refusal is valid: {error}"));
        }
        let reaction = self
            .agent
            .finish_compaction_attempt(finished.clone())
            .map_err(RuntimeError::CompactionRefused)?;
        let operation = self
            .compaction
            .as_mut()
            .ok_or(RuntimeError::CompactionContinuationMissing)?;
        operation.phase = CompactionPhase::Finishing { finished };
        self.begin_transition(reaction, Vec::new(), AfterCommit::FinishCompaction)?;
        Box::pin(self.finish_transition()).await
    }

    pub(super) fn finish_compaction_attempt(&mut self) -> Result<(), RuntimeError> {
        let operation = self
            .compaction
            .take()
            .ok_or(RuntimeError::CompactionContinuationMissing)?;
        let CompactionPhase::Finishing { finished } = &operation.phase else {
            self.compaction = Some(operation);
            return Err(RuntimeError::CompactionContinuationMissing);
        };
        if matches!(&operation.continuation, Continuation::Cancelled) {
            if operation.requested {
                self.report.requested_compaction = Some(RequestedCompactionOutcome::Failed {
                    id: operation.prepared.plan().id().clone(),
                    kind: finished
                        .outcome()
                        .failure()
                        .unwrap_or(CompactionFailure::Cancelled),
                });
            }
            return Ok(());
        }
        if matches!(finished.outcome(), CompactionOutcome::Complete { .. }) {
            let successful_attempt_id = finished.attempt_id().clone();
            let reaction = self
                .agent
                .commit_compaction_checkpoint(
                    operation.prepared.plan().clone(),
                    successful_attempt_id.clone(),
                )
                .map_err(RuntimeError::CompactionRefused)?;
            self.compaction = Some(CompactionOperation {
                phase: CompactionPhase::Publishing,
                ..operation
            });
            return self.begin_transition(reaction, Vec::new(), AfterCommit::PublishCompaction);
        }
        self.resume_after_failed_compaction(operation)
    }

    pub(super) async fn publish_compaction(&mut self) -> Result<(), RuntimeError> {
        let operation = self
            .compaction
            .take()
            .ok_or(RuntimeError::CompactionContinuationMissing)?;
        let CompactionPhase::Publishing = &operation.phase else {
            self.compaction = Some(operation);
            return Err(RuntimeError::CompactionContinuationMissing);
        };
        match operation.continuation {
            Continuation::ModelCall { original, .. } => {
                let call = self
                    .agent
                    .model_call_for_active_step(&original.step_id)
                    .map_err(RuntimeError::CompactionRefused)?;
                self.authorize_model(call)
            }
            Continuation::Requested => {
                self.report.requested_compaction = Some(RequestedCompactionOutcome::Published {
                    id: operation.prepared.plan().id().clone(),
                });
                Ok(())
            }
            Continuation::Cancelled => Ok(()),
        }
    }

    fn resume_after_failed_compaction(
        &mut self,
        mut operation: CompactionOperation,
    ) -> Result<(), RuntimeError> {
        match &operation.continuation {
            Continuation::ModelCall {
                original,
                trigger: CompactionTrigger::TurnStartSoft,
            } => self.authorize_model(original.clone()),
            Continuation::ModelCall { original, .. } => {
                let reaction = self.agent.handle_at(
                    plexmaton_agent::Input::Failed {
                        step_id: original.step_id.clone(),
                        error: ModelError::ContextTooLong,
                    },
                    self.clock.now(),
                );
                operation.phase = CompactionPhase::Failing;
                self.compaction = Some(operation);
                self.begin_transition(reaction, Vec::new(), AfterCommit::CompleteCompactionFailure)
            }
            Continuation::Requested => {
                let kind = match &operation.phase {
                    CompactionPhase::Finishing { finished } => finished.outcome().failure(),
                    _ => None,
                }
                .unwrap_or(CompactionFailure::Unavailable);
                self.report.requested_compaction = Some(RequestedCompactionOutcome::Failed {
                    id: operation.prepared.plan().id().clone(),
                    kind,
                });
                Ok(())
            }
            Continuation::Cancelled => Ok(()),
        }
    }

    fn finish_compaction_failure(
        &mut self,
        call: ModelCall,
        trigger: CompactionTrigger,
    ) -> Result<(), RuntimeError> {
        if trigger == CompactionTrigger::TurnStartSoft {
            self.authorize_model(call)
        } else {
            // The async caller turns hard planning refusal into the ordinary typed step failure.
            self.deferred_compaction_failure = Some(call);
            self.after_commit = Some(AfterCommit::FailCompaction);
            Ok(())
        }
    }

    async fn finish_abnormal_compaction(&mut self) -> Result<(), RuntimeError> {
        self.compaction.take();
        Err(RuntimeError::CompactionFutureFailed)
    }

    pub(super) fn fail_deferred_compaction(&mut self) -> Result<(), RuntimeError> {
        let Some(call) = self.deferred_compaction_failure.take() else {
            return Ok(());
        };
        let reaction = self.agent.handle_at(
            plexmaton_agent::Input::Failed {
                step_id: call.step_id,
                error: ModelError::ContextTooLong,
            },
            self.clock.now(),
        );
        self.begin_transition(reaction, Vec::new(), AfterCommit::CompleteCompactionFailure)
    }

    pub(super) fn complete_compaction_failure(&mut self) -> Result<(), RuntimeError> {
        if self
            .compaction
            .as_ref()
            .is_some_and(|operation| !matches!(operation.phase, CompactionPhase::Failing))
        {
            return Err(RuntimeError::CompactionContinuationMissing);
        }
        self.compaction = None;
        Ok(())
    }

    pub(super) async fn cancel_compaction(&mut self) -> Result<(), RuntimeError> {
        let Some(operation) = self.compaction.as_mut() else {
            return Ok(());
        };
        operation.cancel();
        let cancelled_authorization = match &operation.phase {
            CompactionPhase::Authorizing { attempt_id, input } => {
                Some((attempt_id.clone(), input.clone()))
            }
            _ => None,
        };
        if let Some((attempt_id, input)) = cancelled_authorization {
            let finished = cancelled_before_dispatch(attempt_id, input);
            let reaction = self
                .agent
                .finish_compaction_attempt(finished.clone())
                .map_err(RuntimeError::CompactionRefused)?;
            self.compaction
                .as_mut()
                .unwrap_or_else(|| unreachable!("cancelled authorization remains owned"))
                .phase = CompactionPhase::Finishing { finished };
            return self.begin_transition(reaction, Vec::new(), AfterCommit::FinishCompaction);
        }
        let CompactionPhase::Running {
            cancellation,
            future,
            ..
        } = &mut operation.phase
        else {
            return Ok(());
        };
        cancellation.cancel();
        let result = (&mut *future).await;
        self.compaction_ended(result).await
    }

    pub(super) async fn discard_compaction_after_journal_failure(
        &mut self,
    ) -> Result<(), RuntimeError> {
        let Some(mut operation) = self.compaction.take() else {
            return Ok(());
        };
        let CompactionPhase::Running {
            cancellation,
            future,
            ..
        } = &mut operation.phase
        else {
            return Ok(());
        };
        cancellation.cancel();
        (&mut *future)
            .await
            .map(|_| ())
            .map_err(|_| RuntimeError::CompactionFutureFailed)
    }
}

fn cancelled_before_dispatch(
    attempt_id: RequestAttemptId,
    _input: CompactionInput,
) -> CompactionAttemptFinished {
    let terminal = plexmaton_agent::RequestAttemptTerminal::new(
        attempt_id,
        plexmaton_agent::RequestAttemptTerminalState::NotDispatched {
            outcome: plexmaton_agent::RequestNotDispatchedOutcome::Cancelled,
        },
    )
    .unwrap_or_else(|error| unreachable!("cancelled authorization terminal is valid: {error}"));
    CompactionAttemptFinished::new(
        terminal,
        plexmaton_agent::CompactionInputMode::Verbatim,
        CompactionOutcome::Failed {
            kind: CompactionFailure::Cancelled,
            output: None,
        },
    )
    .unwrap_or_else(|error| unreachable!("cancelled authorization finish is valid: {error}"))
}
