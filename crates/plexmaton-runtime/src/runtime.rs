//! One live agent, its bounded provider channel, and every retained outside operation.

use std::{collections::VecDeque, sync::Arc};

use plexmaton_agent::{
    Agent, Input, ModelCall, ModelStepId, RequestAttemptId, UndeliveredInput, UndeliveredReason,
};
use plexmaton_core::{AgentId, SessionEventEnvelope};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{CleanupFailure, DispatchReport, RuntimeError, RuntimeUpdate};

mod clock;
mod construction;
mod journal;
mod model;
mod terminal;
mod tools;
mod transition;

#[cfg(test)]
pub(crate) use clock::FixedWallClock;
pub(crate) use clock::WallClock;
use model::RetainedModelFuture;
pub(crate) use model::{
    ModelCompletion, ModelDriver, ModelOutput, ModelSignal, ModelTerminalReport,
};
use tools::{ToolResolution, ToolTasks};
use transition::{AfterCommit, PendingCommit};

use journal::JournalWriter;

const PENDING_INPUT_CAPACITY: usize = 32;

struct PendingInput {
    input: Input,
    observed_at: plexmaton_agent::UnixMillis,
    rejected_input: Option<UndeliveredInput>,
    after: AfterCommit,
}

struct PendingModelStart {
    attempt_id: RequestAttemptId,
    call: ModelCall,
}

enum ModelSettlement {
    Running,
    Terminal {
        report: Box<ModelTerminalReport>,
        audit_staged: bool,
        delivery_staged: bool,
    },
    Abnormal {
        delivery_staged: bool,
    },
}

struct ActiveModel {
    attempt_id: RequestAttemptId,
    step_id: ModelStepId,
    cancellation: CancellationToken,
    future: RetainedModelFuture,
    settlement: ModelSettlement,
}

/// Owner of one live agent and every asynchronous operation it starts (LIVE-1).
pub struct LiveRuntime {
    agent_id: AgentId,
    agent: Agent,
    driver: Arc<dyn ModelDriver>,
    pending: VecDeque<SessionEventEnvelope>,
    signals: mpsc::Sender<ModelSignal>,
    signal_rx: mpsc::Receiver<ModelSignal>,
    active: Option<ActiveModel>,
    pending_model_start: Option<PendingModelStart>,
    deferred_model_call: Option<ModelCall>,
    tools: ToolTasks,
    report: DispatchReport,
    journal: Option<JournalWriter>,
    pending_commit: Option<PendingCommit>,
    after_commit: Option<AfterCommit>,
    pending_inputs: VecDeque<PendingInput>,
    journal_failed: bool,
    shutting_down: bool,
    clock: Arc<dyn WallClock>,
}

impl LiveRuntime {
    /// Gives one addressed input to the owned agent and performs every resulting effect.
    pub async fn submit(
        &mut self,
        to: AgentId,
        input: Input,
    ) -> Result<DispatchReport, RuntimeError> {
        if to != self.agent_id {
            return Err(RuntimeError::WrongAgent {
                expected: self.agent_id.clone(),
                received: to,
            });
        }
        if self.shutting_down {
            if let Some(input) = rejected_user_input(&input, UndeliveredReason::Shutdown) {
                self.report.undelivered.push(input);
                return Ok(self.take_report());
            }
            return Err(RuntimeError::ShuttingDown);
        }
        if self.pending_inputs.len() == PENDING_INPUT_CAPACITY {
            if let Some(input) = rejected_user_input(&input, UndeliveredReason::QueueFull) {
                self.report.undelivered.push(input);
                return Ok(self.take_report());
            }
            return Err(RuntimeError::RuntimeInputQueueFull);
        }
        let rejected_input = match &input {
            Input::Submitted { text } | Input::Steered { text } => Some(UndeliveredInput {
                text: text.clone(),
                reason: UndeliveredReason::PersistenceFailed,
            }),
            Input::Streamed { .. }
            | Input::Failed { .. }
            | Input::ToolAdmissionResolved(_)
            | Input::ToolFinished { .. }
            | Input::ApprovalDecided { .. }
            | Input::Interrupted
            | Input::ShuttingDown => None,
        };
        let after = if matches!(&input, Input::Interrupted) {
            AfterCommit::Interrupt
        } else {
            AfterCommit::None
        };
        let observed_at = self.clock.now();
        self.pending_inputs.push_back(PendingInput {
            input,
            observed_at,
            rejected_input,
            after,
        });
        if let Err(error) = self.finish_pending_inputs().await {
            if self.journal_failed {
                self.finish_failed_owners().await;
            }
            return Err(error);
        }
        if self.journal_failed {
            self.finish_failed_owners().await;
            return if self.report.is_empty() {
                Err(RuntimeError::JournalRequiresReopen)
            } else {
                Ok(self.take_report())
            };
        }
        Ok(self.take_report())
    }

    /// Returns an event already produced without waiting for provider traffic.
    pub fn try_next_event(&mut self) -> Option<SessionEventEnvelope> {
        self.pending.pop_front()
    }

    /// Waits cancellation-safely for the next semantic event.
    ///
    /// Composition roots should prefer [`Self::next_update`] so non-event ownership reports cannot
    /// be missed. This event-only surface leaves such a report available through `take_report` and
    /// returns [`RuntimeError::DispatchReportPending`].
    pub async fn next_event(&mut self) -> Result<Option<SessionEventEnvelope>, RuntimeError> {
        match self.next_update().await? {
            RuntimeUpdate::Event(event) => Ok(Some(event)),
            RuntimeUpdate::Finished => Ok(None),
            RuntimeUpdate::Report(report) => {
                self.report = report;
                Err(RuntimeError::DispatchReportPending)
            }
        }
    }

    /// Waits cancellation-safely for the next event, non-event report, or final completion.
    pub async fn next_update(&mut self) -> Result<RuntimeUpdate, RuntimeError> {
        if let Err(error) = self.finish_pending_inputs().await {
            if self.journal_failed {
                self.finish_failed_owners().await;
            }
            return Err(error);
        }
        loop {
            if self.journal_failed {
                self.finish_failed_owners().await;
            }
            if let Some(event) = self.try_next_event() {
                return Ok(RuntimeUpdate::Event(event));
            }
            if !self.report.is_empty() {
                return Ok(RuntimeUpdate::Report(self.take_report()));
            }
            if self.shutting_down && !self.has_active_work() {
                return Ok(RuntimeUpdate::Finished);
            }
            let transition = match self.wait_for_work().await {
                WaitOutcome::Signal(Some(signal)) => self.apply_signal(signal).await,
                WaitOutcome::Signal(None) => return Ok(RuntimeUpdate::Finished),
                WaitOutcome::ModelEnded(result) => self.model_ended(result).await,
                WaitOutcome::Tool(Ok(Some(resolution))) => {
                    self.apply_tool_resolution(resolution).await
                }
                WaitOutcome::Tool(Ok(None)) => Ok(()),
                WaitOutcome::Tool(Err(error)) => Err(error),
            };
            if let Err(error) = transition {
                if self.journal_failed {
                    self.finish_failed_owners().await;
                }
                return Err(error);
            }
        }
    }

    /// Begins orderly shutdown, settles the agent first, then cancels and joins all owned work.
    ///
    /// Cancellation of this future does not make shutdown look complete: calling it again resumes
    /// the retained provider and tool cleanup.
    pub async fn shutdown(&mut self) -> Result<DispatchReport, RuntimeError> {
        if let Err(error) = self.finish_pending_inputs().await {
            return self.shutdown_after_journal_failure(error).await;
        }
        if self.journal_failed {
            return self
                .shutdown_after_journal_failure(RuntimeError::JournalRequiresReopen)
                .await;
        }
        if !self.shutting_down {
            self.shutting_down = true;
            if let Err(error) = self
                .apply_agent_input(Input::ShuttingDown, None, AfterCommit::Shutdown)
                .await
            {
                return self.shutdown_after_journal_failure(error).await;
            }
        }
        if self.journal_failed {
            return self
                .shutdown_after_journal_failure(RuntimeError::JournalRequiresReopen)
                .await;
        }
        if let Err(error) = self.finish_transition().await {
            return self.shutdown_after_journal_failure(error).await;
        }
        if let Some(journal) = &mut self.journal {
            journal
                .shutdown()
                .await
                .map_err(|_| RuntimeError::JournalWriterUnavailable)?;
        }
        Ok(self.take_report())
    }

    async fn shutdown_after_journal_failure(
        &mut self,
        error: RuntimeError,
    ) -> Result<DispatchReport, RuntimeError> {
        self.shutting_down = true;
        self.finish_failed_owners().await;
        if self.report.persistence_failure.is_some() {
            Ok(self.take_report())
        } else {
            Err(error)
        }
    }

    async fn finish_failed_owners(&mut self) {
        self.pending_model_start = None;
        self.deferred_model_call = None;
        self.after_commit = None;
        let provider = self.discard_active_after_journal_failure().await;
        let tools = self.tools.cancel_and_join().await;
        let writer = match &mut self.journal {
            Some(journal) => journal
                .shutdown()
                .await
                .map_err(|_| RuntimeError::JournalWriterUnavailable),
            None => Ok(()),
        };
        if provider.is_err() {
            self.report.cleanup_failures.push(CleanupFailure::Provider);
        }
        if tools.is_err() {
            self.report.cleanup_failures.push(CleanupFailure::Tools);
        }
        if writer.is_err() {
            self.report
                .cleanup_failures
                .push(CleanupFailure::JournalWriter);
        }
    }

    /// Whether this runtime still owns a provider operation.
    #[must_use]
    pub fn has_active_model(&self) -> bool {
        self.active.is_some()
    }

    /// Sole agent identity accepted by this runtime and named by its non-event reports.
    #[must_use]
    pub const fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    /// Whether this runtime still owns a durable transition, provider, admission, or execution.
    #[must_use]
    pub fn has_active_work(&self) -> bool {
        !self.pending_inputs.is_empty()
            || self.pending_commit.is_some()
            || self.after_commit.is_some()
            || self.pending_model_start.is_some()
            || self.deferred_model_call.is_some()
            || self.active.is_some()
            || !self.tools.is_empty()
    }

    /// Takes non-event delivery results accumulated while provider traffic was processed.
    pub fn take_report(&mut self) -> DispatchReport {
        std::mem::take(&mut self.report)
    }

    async fn wait_for_work(&mut self) -> WaitOutcome {
        match (self.active.as_mut(), self.tools.is_empty()) {
            (Some(active), false) => tokio::select! {
                biased;
                signal = self.signal_rx.recv() => WaitOutcome::Signal(signal),
                ended = &mut active.future => WaitOutcome::ModelEnded(ended),
                tool = self.tools.next() => WaitOutcome::Tool(tool),
            },
            (Some(active), true) => tokio::select! {
                biased;
                signal = self.signal_rx.recv() => WaitOutcome::Signal(signal),
                ended = &mut active.future => WaitOutcome::ModelEnded(ended),
            },
            (None, false) => tokio::select! {
                biased;
                signal = self.signal_rx.recv() => WaitOutcome::Signal(signal),
                tool = self.tools.next() => WaitOutcome::Tool(tool),
            },
            (None, true) => WaitOutcome::Signal(self.signal_rx.recv().await),
        }
    }

    async fn apply_tool_resolution(
        &mut self,
        resolution: ToolResolution,
    ) -> Result<(), RuntimeError> {
        let input = match resolution {
            ToolResolution::Admission(outcome) => Input::ToolAdmissionResolved(outcome),
            ToolResolution::Execution { call_id, result } => {
                Input::ToolFinished { call_id, result }
            }
        };
        self.apply_agent_input(input, None, AfterCommit::None).await
    }
}

fn rejected_user_input(input: &Input, reason: UndeliveredReason) -> Option<UndeliveredInput> {
    match input {
        Input::Submitted { text } | Input::Steered { text } => Some(UndeliveredInput {
            text: text.clone(),
            reason,
        }),
        _ => None,
    }
}

impl Drop for LiveRuntime {
    fn drop(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancellation.cancel();
        }
    }
}

enum WaitOutcome {
    Signal(Option<ModelSignal>),
    ModelEnded(Result<ModelTerminalReport, ()>),
    Tool(Result<Option<ToolResolution>, RuntimeError>),
}

#[cfg(test)]
mod tests;
