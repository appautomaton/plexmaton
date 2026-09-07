//! Provider-operation future retained across cancellation of an event poll.

use plexmaton_agent::{
    Input, ModelCall, ModelDeliveryRefusal, ModelError, ModelEvent, ModelStepId, RequestAttemptId,
    RequestAttemptTerminal, RequestAttemptTerminalState, RequestDispatchedOutcome,
    RequestNotDispatchedOutcome, StopReason, UndeliveredModelInput,
};
use tokio_util::sync::CancellationToken;

use super::{
    ActiveModel, LiveRuntime, ModelSettlement, PendingModelStart, RetainedFuture,
    transition::AfterCommit,
};
use crate::RuntimeError;

mod driver;
pub(crate) use driver::ModelDriver;

#[derive(Debug)]
pub(crate) struct ModelSignal {
    pub(crate) attempt_id: RequestAttemptId,
    pub(crate) step_id: ModelStepId,
    pub(crate) output: ModelOutput,
}

#[derive(Debug)]
pub(crate) struct ModelOutput(ModelEvent);

impl ModelOutput {
    pub(crate) fn from_event(event: ModelEvent) -> Result<Self, ModelEvent> {
        match event {
            ModelEvent::Usage(_) | ModelEvent::Stopped(_) => Err(event),
            _ => Ok(Self(event)),
        }
    }

    pub(crate) fn is_first_output(&self) -> bool {
        match &self.0 {
            ModelEvent::TextDelta { delta, .. } | ModelEvent::ReasoningDelta { delta, .. } => {
                !delta.is_empty()
            }
            ModelEvent::Replay { .. } | ModelEvent::Called { .. } => true,
            ModelEvent::Usage(_) | ModelEvent::Stopped(_) => {
                unreachable!("ModelOutput excludes terminal and accounting events")
            }
        }
    }

    pub(crate) fn into_event(self) -> ModelEvent {
        self.0
    }
}

#[derive(Clone, Debug)]
pub(crate) enum ModelCompletion {
    Stopped(StopReason),
    Failed(ModelError),
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) struct ModelTerminalReport {
    pub(crate) step_id: ModelStepId,
    pub(crate) terminal: RequestAttemptTerminal,
    pub(crate) completion: ModelCompletion,
}

impl ModelTerminalReport {
    pub(crate) fn new(
        step_id: ModelStepId,
        terminal: RequestAttemptTerminal,
        completion: ModelCompletion,
    ) -> Self {
        debug_assert!(completion_matches(terminal.terminal(), &completion));
        Self {
            step_id,
            terminal,
            completion,
        }
    }

    fn is_consistent_with(&self, active: &ActiveModel) -> bool {
        self.step_id == active.step_id
            && self.terminal.attempt_id() == &active.attempt_id
            && completion_matches(self.terminal.terminal(), &self.completion)
    }
}

fn completion_matches(
    terminal: &RequestAttemptTerminalState,
    completion: &ModelCompletion,
) -> bool {
    match (terminal, completion) {
        (
            RequestAttemptTerminalState::NotDispatched {
                outcome: RequestNotDispatchedOutcome::Cancelled,
            }
            | RequestAttemptTerminalState::Dispatched {
                outcome: RequestDispatchedOutcome::Cancelled,
                ..
            },
            ModelCompletion::Cancelled,
        ) => true,
        (
            RequestAttemptTerminalState::NotDispatched {
                outcome:
                    RequestNotDispatchedOutcome::PreparationFailed
                    | RequestNotDispatchedOutcome::EncodingFailed,
            },
            ModelCompletion::Failed(_),
        ) => true,
        (
            RequestAttemptTerminalState::Dispatched {
                outcome: RequestDispatchedOutcome::Completed { stop_reason },
                ..
            },
            ModelCompletion::Stopped(completion),
        ) => stop_reason == completion,
        (
            RequestAttemptTerminalState::Dispatched {
                outcome:
                    RequestDispatchedOutcome::TransportFailed
                    | RequestDispatchedOutcome::ProviderFailed
                    | RequestDispatchedOutcome::RateLimited
                    | RequestDispatchedOutcome::ContextTooLong
                    | RequestDispatchedOutcome::Malformed,
                ..
            },
            ModelCompletion::Failed(_),
        ) => true,
        _ => false,
    }
}

impl LiveRuntime {
    pub(super) async fn apply_signal(&mut self, signal: ModelSignal) -> Result<(), RuntimeError> {
        self.apply_signal_inner(signal, true).await
    }

    async fn apply_signal_inner(
        &mut self,
        signal: ModelSignal,
        finish_after_commit: bool,
    ) -> Result<(), RuntimeError> {
        let ModelSignal {
            attempt_id,
            step_id,
            output,
        } = signal;
        if self.active_matches(&attempt_id, &step_id) {
            let input = Input::Streamed {
                step_id,
                event: output.into_event(),
            };
            if finish_after_commit {
                self.apply_agent_input(input, None, AfterCommit::None).await
            } else {
                self.apply_agent_input_during_join(input).await
            }
        } else {
            let reason = match &self.active {
                None => ModelDeliveryRefusal::NoActiveStep,
                Some(active) if active.attempt_id != attempt_id => {
                    ModelDeliveryRefusal::WrongAttempt {
                        expected: active.attempt_id.clone(),
                        received: attempt_id,
                    }
                }
                Some(active) => ModelDeliveryRefusal::WrongStep {
                    expected: active.step_id.clone(),
                },
            };
            self.report
                .undelivered_model
                .push(UndeliveredModelInput { step_id, reason });
            Ok(())
        }
    }

    pub(super) async fn model_ended(
        &mut self,
        result: Result<ModelTerminalReport, ()>,
    ) -> Result<(), RuntimeError> {
        self.drain_ready_signals().await?;
        if self.journal_failed {
            return Ok(());
        }
        if self.active.is_none() {
            return Ok(());
        }
        self.retain_model_result(result);
        let needs_audit = matches!(
            self.active.as_ref().map(|active| &active.settlement),
            Some(ModelSettlement::Terminal {
                audit_staged: false,
                ..
            })
        );
        if needs_audit {
            self.stage_attempt_terminal(AfterCommit::SettleModel)?;
        } else if matches!(
            self.active.as_ref().map(|active| &active.settlement),
            Some(ModelSettlement::Abnormal { .. })
        ) {
            if self.after_commit.is_some() {
                return Err(RuntimeError::ModelSettlementAlreadyPending);
            }
            self.after_commit = Some(AfterCommit::SettleModel);
        }
        self.finish_transition().await
    }

    pub(super) fn retain_model_result(&mut self, result: Result<ModelTerminalReport, ()>) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if !matches!(active.settlement, ModelSettlement::Running) {
            return;
        }
        active.settlement = match result {
            Ok(report) if report.is_consistent_with(active) => ModelSettlement::Terminal {
                report: Box::new(report),
                audit_staged: false,
                delivery_staged: false,
            },
            Ok(_) | Err(()) => ModelSettlement::Abnormal {
                delivery_staged: false,
            },
        };
    }

    pub(super) fn authorize_model(&mut self, call: ModelCall) -> Result<(), RuntimeError> {
        if self.shutdown_state != super::ShutdownState::Open {
            return Err(RuntimeError::ShuttingDown);
        }
        if let Some(active) = &self.active {
            let completion_is_committing = matches!(
                &active.settlement,
                ModelSettlement::Terminal {
                    delivery_staged: true,
                    ..
                } | ModelSettlement::Abnormal {
                    delivery_staged: true
                }
            );
            if completion_is_committing && self.deferred_model_call.is_none() {
                self.deferred_model_call = Some(call);
                return Ok(());
            }
            return Err(RuntimeError::ModelAlreadyActive {
                active: active.step_id.clone(),
                requested: call.step_id,
            });
        }
        if let Some(pending) = &self.pending_model_start {
            return Err(RuntimeError::ModelAlreadyActive {
                active: pending.call.step_id.clone(),
                requested: call.step_id,
            });
        }
        if !self.tools.is_empty() {
            return Err(RuntimeError::ModelStartedWithToolWork);
        }
        let step_id = call.step_id.clone();
        let (attempt_id, reaction) = self
            .agent
            .authorize_request_attempt(
                step_id,
                self.driver.request_environment().clone(),
                self.clock.now(),
            )
            .map_err(RuntimeError::RequestAttemptRefused)?;
        self.pending_model_start = Some(PendingModelStart {
            attempt_id,
            call,
            cancelled: false,
        });
        if let Err(error) = self.begin_transition(reaction, Vec::new(), AfterCommit::StartModel) {
            self.pending_model_start = None;
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn start_authorized_model(&mut self) -> Result<(), RuntimeError> {
        let pending = self
            .pending_model_start
            .as_ref()
            .ok_or(RuntimeError::MissingAuthorizedModelStart)?;
        if self.shutdown_state != super::ShutdownState::Open && !pending.cancelled {
            return Err(RuntimeError::ShuttingDown);
        }
        if let Some(active) = &self.active {
            return Err(RuntimeError::ModelAlreadyActive {
                active: active.step_id.clone(),
                requested: pending.call.step_id.clone(),
            });
        }
        if !self.tools.is_empty() {
            return Err(RuntimeError::ModelStartedWithToolWork);
        }
        let pending = self
            .pending_model_start
            .take()
            .unwrap_or_else(|| unreachable!("the authorized model start remains owned"));
        if pending.cancelled {
            let terminal = RequestAttemptTerminal::new(
                pending.attempt_id,
                RequestAttemptTerminalState::NotDispatched {
                    outcome: RequestNotDispatchedOutcome::Cancelled,
                },
            )
            .unwrap_or_else(|error| {
                unreachable!("cancelled pending model terminal is valid: {error}")
            });
            let reaction = self
                .agent
                .finish_request_attempt(&terminal)
                .map_err(RuntimeError::RequestAttemptRefused)?;
            return self.begin_transition(reaction, Vec::new(), AfterCommit::None);
        }
        let step_id = pending.call.step_id.clone();
        let cancellation = CancellationToken::new();
        let future = self.driver.drive(
            pending.attempt_id.clone(),
            pending.call,
            self.signals.clone(),
            cancellation.child_token(),
        );
        self.active = Some(ActiveModel {
            attempt_id: pending.attempt_id,
            step_id,
            cancellation,
            future: RetainedFuture::new(future),
            settlement: ModelSettlement::Running,
        });
        Ok(())
    }

    pub(super) fn cancel_pending_model_before_dispatch(&mut self) {
        if let Some(pending) = self.pending_model_start.as_mut() {
            pending.cancelled = true;
        }
    }

    fn stage_attempt_terminal(&mut self, after: AfterCommit) -> Result<(), RuntimeError> {
        let terminal = match self.active.as_ref().map(|active| &active.settlement) {
            Some(ModelSettlement::Terminal {
                report,
                audit_staged: false,
                ..
            }) => report.terminal.clone(),
            _ => return Ok(()),
        };
        let reaction = self
            .agent
            .finish_request_attempt(&terminal)
            .map_err(RuntimeError::RequestAttemptRefused)?;
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        let ModelSettlement::Terminal { audit_staged, .. } = &mut active.settlement else {
            return Ok(());
        };
        *audit_staged = true;
        self.begin_transition(reaction, Vec::new(), after)
    }

    pub(super) async fn finish_attempt_audit_during_owner_action(
        &mut self,
    ) -> Result<(), RuntimeError> {
        self.stage_attempt_terminal(AfterCommit::None)?;
        self.finish_pending_transition().await
    }

    pub(super) async fn settle_model_completion(&mut self) -> Result<(), RuntimeError> {
        let context_recovery = self.active.as_ref().and_then(|active| {
            let ModelSettlement::Terminal {
                report,
                audit_staged: true,
                delivery_staged: false,
            } = &active.settlement
            else {
                return None;
            };
            let no_output = matches!(
                report.terminal.terminal(),
                RequestAttemptTerminalState::Dispatched { timing, .. }
                    if timing.first_output_after_ms().is_none()
            );
            (no_output
                && matches!(
                    report.completion,
                    ModelCompletion::Failed(ModelError::ContextTooLong)
                ))
            .then(|| active.step_id.clone())
        });
        if let Some(step_id) = context_recovery
            && self.shutdown_state == super::ShutdownState::Open
            && self.compaction_budget.take_context_recovery(&step_id)
        {
            let call = self
                .agent
                .model_call_for_active_step(&step_id)
                .map_err(RuntimeError::CompactionRefused)?;
            self.active = None;
            return self.begin_compaction(call, super::compaction::CompactionTrigger::ContextError);
        }
        let input = {
            let active = self
                .active
                .as_mut()
                .ok_or(RuntimeError::MissingActiveModelSettlement)?;
            match &mut active.settlement {
                ModelSettlement::Running => {
                    return Err(RuntimeError::MissingActiveModelSettlement);
                }
                ModelSettlement::Terminal {
                    report,
                    audit_staged,
                    delivery_staged,
                } => {
                    if !*audit_staged {
                        return Err(RuntimeError::MissingRequestAttemptAudit);
                    }
                    if *delivery_staged {
                        None
                    } else {
                        *delivery_staged = true;
                        completion_input(active.step_id.clone(), &report.completion)
                    }
                }
                ModelSettlement::Abnormal { delivery_staged } => {
                    if *delivery_staged {
                        None
                    } else {
                        *delivery_staged = true;
                        Some(Input::Failed {
                            step_id: active.step_id.clone(),
                            error: ModelError::Transport {
                                message: "provider future terminated unexpectedly".to_owned(),
                            },
                        })
                    }
                }
            }
        };
        if let Some(input) = input {
            self.apply_agent_input_during_join(input).await?;
        }
        self.active = None;
        Ok(())
    }

    fn active_matches(&self, attempt_id: &RequestAttemptId, step_id: &ModelStepId) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.attempt_id == *attempt_id && active.step_id == *step_id)
    }

    pub(super) async fn drain_ready_signals(&mut self) -> Result<(), RuntimeError> {
        while let Ok(signal) = self.signal_rx.try_recv() {
            self.apply_signal_inner(signal, false).await?;
            if self.journal_failed {
                break;
            }
        }
        Ok(())
    }
}

fn completion_input(step_id: ModelStepId, completion: &ModelCompletion) -> Option<Input> {
    match completion {
        ModelCompletion::Stopped(reason) => Some(Input::Streamed {
            step_id,
            event: ModelEvent::Stopped(*reason),
        }),
        ModelCompletion::Failed(error) => Some(Input::Failed {
            step_id,
            error: error.clone(),
        }),
        ModelCompletion::Cancelled => None,
    }
}
