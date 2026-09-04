//! Provider-operation future retained across cancellation of an event poll.

use std::{
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{FutureExt as _, future::BoxFuture};
use plexmaton_agent::{
    Input, ModelCall, ModelError, ModelEvent, ModelStepId, Reaction, RequestAttemptId,
    RequestAttemptTerminal, RequestAttemptTerminalState, RequestDispatchedOutcome,
    RequestEnvironment, RequestNotDispatchedOutcome, StopReason,
};
use plexmaton_core::TokenUsage;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{
    ActiveModel, LiveRuntime,
    transition::{AfterCommit, merge_reaction},
};
use crate::RuntimeError;

pub(crate) trait ModelDriver: Send + Sync + 'static {
    #[allow(dead_code)] // Consumed by the authorization commit integration stacked after this seam.
    fn request_environment(&self) -> &RequestEnvironment;

    fn drive(
        &self,
        attempt_id: RequestAttemptId,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, ModelTerminalReport>;
}

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

/// A completed future may be observed by a cancelled outer poll and awaited again during cleanup.
pub(super) struct RetainedModelFuture {
    future: Option<BoxFuture<'static, Result<ModelTerminalReport, ()>>>,
    result: Option<Result<ModelTerminalReport, ()>>,
}

impl RetainedModelFuture {
    pub(super) fn new(future: BoxFuture<'static, ModelTerminalReport>) -> Self {
        let future = AssertUnwindSafe(future)
            .catch_unwind()
            .map(|result| result.map_err(|_| ()))
            .boxed();
        Self {
            future: Some(future),
            result: None,
        }
    }
}

impl Future for RetainedModelFuture {
    type Output = Result<ModelTerminalReport, ()>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(result) = &this.result {
            return Poll::Ready(result.clone());
        }
        let Some(future) = this.future.as_mut() else {
            return Poll::Ready(Err(()));
        };
        match future.as_mut().poll(context) {
            Poll::Ready(result) => {
                this.future = None;
                this.result = Some(result.clone());
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
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
        let Some(active) = self.active.as_ref() else {
            return Ok(());
        };
        let report = match result {
            Ok(report)
                if report.step_id == active.step_id
                    && report.terminal.attempt_id() == &active.attempt_id =>
            {
                report
            }
            Ok(_) | Err(()) => {
                let step_id = active.step_id.clone();
                self.active.take();
                return self
                    .fail_owned_step(step_id, "provider future terminated unexpectedly")
                    .await;
            }
        };
        let step_id = active.step_id.clone();
        let usage = terminal_usage(&report.terminal);
        self.apply_agent_input(
            Input::Streamed {
                step_id: step_id.clone(),
                event: ModelEvent::Usage(usage),
            },
            None,
            AfterCommit::None,
        )
        .await?;
        self.active.take();
        match report.completion {
            ModelCompletion::Stopped(reason) => {
                self.apply_agent_input(
                    Input::Streamed {
                        step_id,
                        event: ModelEvent::Stopped(reason),
                    },
                    None,
                    AfterCommit::None,
                )
                .await?;
            }
            ModelCompletion::Failed(error) => {
                self.apply_agent_input(Input::Failed { step_id, error }, None, AfterCommit::None)
                    .await?;
            }
            ModelCompletion::Cancelled => {}
        }
        Ok(())
    }

    async fn fail_owned_step(
        &mut self,
        step_id: ModelStepId,
        message: &'static str,
    ) -> Result<(), RuntimeError> {
        self.apply_agent_input(
            Input::Failed {
                step_id,
                error: ModelError::Transport {
                    message: message.to_owned(),
                },
            },
            None,
            AfterCommit::None,
        )
        .await
    }

    pub(super) fn spawn_model(&mut self, call: ModelCall) -> Result<(), RuntimeError> {
        if self.shutting_down {
            return Err(RuntimeError::ShuttingDown);
        }
        if let Some(active) = &self.active {
            return Err(RuntimeError::ModelAlreadyActive {
                active: active.step_id.clone(),
                requested: call.step_id,
            });
        }
        if !self.tools.is_empty() {
            return Err(RuntimeError::ModelStartedWithToolWork);
        }
        let step_id = call.step_id.clone();
        let attempt_id = current_attempt_id(&step_id);
        let cancellation = CancellationToken::new();
        let future = self.driver.drive(
            attempt_id.clone(),
            call,
            self.signals.clone(),
            cancellation.child_token(),
        );
        self.active = Some(ActiveModel {
            attempt_id,
            step_id,
            cancellation,
            future: RetainedModelFuture::new(future),
        });
        Ok(())
    }

    pub(super) fn stage_missing_usage_at(
        &mut self,
        reaction: &mut Reaction,
        observed_at: plexmaton_agent::UnixMillis,
    ) {
        let missing = self.active.as_ref().map(|active| active.step_id.clone());
        let Some(step_id) = missing else {
            return;
        };
        merge_reaction(
            reaction,
            self.agent.handle_at(
                Input::Streamed {
                    step_id,
                    event: ModelEvent::Usage(TokenUsage::Unavailable),
                },
                observed_at,
            ),
        );
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

fn current_attempt_id(step_id: &ModelStepId) -> RequestAttemptId {
    RequestAttemptId::new(format!(
        "{}-step-{}-attempt-1",
        step_id.turn_id(),
        step_id.index()
    ))
    .unwrap_or_else(|error| unreachable!("bounded step identity forms an attempt id: {error}"))
}

fn terminal_usage(terminal: &RequestAttemptTerminal) -> TokenUsage {
    match terminal.terminal() {
        RequestAttemptTerminalState::Dispatched { usage, .. } => usage.clone(),
        RequestAttemptTerminalState::NotDispatched { .. } => TokenUsage::Unavailable,
    }
}
