//! Provider-operation future retained across cancellation of an event poll.

use std::{
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{FutureExt as _, future::BoxFuture};
use plexmaton_agent::{Input, ModelCall, ModelError, ModelEvent, ModelStepId, Reaction};
use plexmaton_core::TokenUsage;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{
    ActiveModel, LiveRuntime,
    terminal::QueuedTerminal,
    transition::{AfterCommit, merge_reaction},
};
use crate::RuntimeError;

pub(crate) trait ModelDriver: Send + Sync + 'static {
    fn drive(
        &self,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, ()>;
}

#[derive(Debug)]
pub(crate) enum ModelSignal {
    Event {
        step_id: ModelStepId,
        event: ModelEvent,
    },
    Terminal {
        step_id: ModelStepId,
        event: ModelEvent,
    },
    Failed {
        step_id: ModelStepId,
        error: ModelError,
    },
}

/// A completed future may be observed by a cancelled outer poll and awaited again during cleanup.
pub(super) struct RetainedModelFuture {
    future: Option<BoxFuture<'static, Result<(), ()>>>,
    result: Option<Result<(), ()>>,
}

impl RetainedModelFuture {
    pub(super) fn new(future: BoxFuture<'static, ()>) -> Self {
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
    type Output = Result<(), ()>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(result) = this.result {
            return Poll::Ready(result);
        }
        let Some(future) = this.future.as_mut() else {
            return Poll::Ready(Err(()));
        };
        match future.as_mut().poll(context) {
            Poll::Ready(result) => {
                this.future = None;
                this.result = Some(result);
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
        match signal {
            ModelSignal::Event { step_id, event } => {
                if self.active_matches(&step_id)
                    && matches!(event, ModelEvent::Usage(_))
                    && let Some(active) = &mut self.active
                {
                    active.usage_reported = true;
                }
                let input = Input::Streamed { step_id, event };
                if finish_after_commit {
                    self.apply_agent_input(input, None, AfterCommit::None).await
                } else {
                    self.apply_agent_input_during_join(input).await
                }
            }
            ModelSignal::Terminal { step_id, event } => {
                let terminal = QueuedTerminal::Streamed(event);
                if self.active_matches(&step_id) {
                    self.queue_terminal(step_id, terminal, finish_after_commit)
                        .await
                } else {
                    self.deliver_terminal(step_id, terminal, finish_after_commit)
                        .await
                }
            }
            ModelSignal::Failed { step_id, error } => {
                let terminal = QueuedTerminal::Failed(error);
                if self.active_matches(&step_id) {
                    self.queue_terminal(step_id, terminal, finish_after_commit)
                        .await
                } else {
                    self.deliver_terminal(step_id, terminal, finish_after_commit)
                        .await
                }
            }
        }
    }

    pub(super) async fn model_ended(&mut self, result: Result<(), ()>) -> Result<(), RuntimeError> {
        self.drain_ready_signals().await?;
        if self.journal_failed {
            return Ok(());
        }
        let Some((step_id, usage_reported)) = self
            .active
            .as_ref()
            .map(|active| (active.step_id.clone(), active.usage_reported))
        else {
            return Ok(());
        };
        self.supply_missing_usage_for(step_id, usage_reported, true)
            .await?;
        let active = self
            .active
            .take()
            .unwrap_or_else(|| unreachable!("the provider owner remains retained across awaits"));
        if result.is_err() {
            return self
                .fail_owned_step(active.step_id, "provider future terminated unexpectedly")
                .await;
        }
        match active.terminal {
            Some(terminal) => self.deliver_terminal(active.step_id, terminal, true).await,
            None => {
                self.fail_owned_step(
                    active.step_id,
                    "provider future ended without terminal output",
                )
                .await
            }
        }
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
        let cancellation = CancellationToken::new();
        let future = self
            .driver
            .drive(call, self.signals.clone(), cancellation.child_token());
        self.active = Some(ActiveModel {
            step_id,
            cancellation,
            future: RetainedModelFuture::new(future),
            usage_reported: false,
            terminal: None,
        });
        Ok(())
    }

    pub(super) async fn supply_missing_usage(&mut self) -> Result<(), RuntimeError> {
        let missing = self
            .active
            .as_ref()
            .map(|active| (active.step_id.clone(), active.usage_reported));
        if let Some((step_id, reported)) = missing {
            self.supply_missing_usage_for(step_id, reported, true)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn supply_missing_usage_during_join(&mut self) -> Result<(), RuntimeError> {
        let missing = self
            .active
            .as_ref()
            .map(|active| (active.step_id.clone(), active.usage_reported));
        if let Some((step_id, reported)) = missing {
            self.supply_missing_usage_for(step_id, reported, false)
                .await?;
        }
        Ok(())
    }

    pub(super) fn stage_missing_usage(&mut self, reaction: &mut Reaction) {
        let missing = self
            .active
            .as_ref()
            .filter(|active| !active.usage_reported)
            .map(|active| active.step_id.clone());
        let Some(step_id) = missing else {
            return;
        };
        if let Some(active) = &mut self.active {
            active.usage_reported = true;
        }
        merge_reaction(
            reaction,
            self.agent.handle(Input::Streamed {
                step_id,
                event: ModelEvent::Usage(TokenUsage::Unavailable),
            }),
        );
    }

    async fn supply_missing_usage_for(
        &mut self,
        step_id: ModelStepId,
        reported: bool,
        finish_after_commit: bool,
    ) -> Result<(), RuntimeError> {
        if reported {
            return Ok(());
        }
        if let Some(active) = &mut self.active
            && active.step_id == step_id
        {
            active.usage_reported = true;
        }
        let input = Input::Streamed {
            step_id,
            event: ModelEvent::Usage(TokenUsage::Unavailable),
        };
        if finish_after_commit {
            self.apply_agent_input(input, None, AfterCommit::None).await
        } else {
            self.apply_agent_input_during_join(input).await
        }
    }

    fn active_matches(&self, step_id: &ModelStepId) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.step_id == *step_id)
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
