//! Terminal provider outcomes held until their exact retained future has settled.

use plexmaton_agent::{Input, ModelError, ModelEvent, ModelStepId};

use super::LiveRuntime;
use crate::RuntimeError;

pub(super) enum QueuedTerminal {
    Streamed(ModelEvent),
    Failed(ModelError),
}

impl LiveRuntime {
    pub(super) async fn queue_terminal(
        &mut self,
        step_id: ModelStepId,
        terminal: QueuedTerminal,
        finish_after_commit: bool,
    ) -> Result<(), RuntimeError> {
        let active = self
            .active
            .as_mut()
            .unwrap_or_else(|| unreachable!("the matching active task still exists"));
        if active.terminal.is_some() {
            return Err(RuntimeError::DuplicateModelTerminal(step_id));
        }
        active.terminal = Some(terminal);
        if finish_after_commit {
            self.supply_missing_usage().await
        } else {
            self.supply_missing_usage_during_join().await
        }
    }

    pub(super) async fn deliver_terminal(
        &mut self,
        step_id: ModelStepId,
        terminal: QueuedTerminal,
        finish_after_commit: bool,
    ) -> Result<(), RuntimeError> {
        let input = match terminal {
            QueuedTerminal::Streamed(event) => Input::Streamed { step_id, event },
            QueuedTerminal::Failed(error) => Input::Failed { step_id, error },
        };
        if finish_after_commit {
            self.apply_agent_input(input, None, super::AfterCommit::None)
                .await
        } else {
            self.apply_agent_input_during_join(input).await
        }
    }

    pub(super) async fn cancel_active(&mut self) -> Result<(), RuntimeError> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        active.cancellation.cancel();
        let step_id = active.step_id.clone();
        let joined = (&mut active.future)
            .await
            .map_err(|_| RuntimeError::ProviderFutureFailed(step_id.clone()));
        Box::pin(self.drain_ready_signals()).await?;
        let active = self
            .active
            .take()
            .unwrap_or_else(|| unreachable!("the awaited active task is still owned"));
        joined?;
        if let Some(terminal) = active.terminal {
            Box::pin(self.deliver_terminal(step_id, terminal, false)).await?;
        }
        Ok(())
    }

    pub(super) async fn discard_active_after_journal_failure(
        &mut self,
    ) -> Result<(), RuntimeError> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        active.cancellation.cancel();
        let step_id = active.step_id.clone();
        let joined = (&mut active.future)
            .await
            .map_err(|_| RuntimeError::ProviderFutureFailed(step_id));
        self.active.take();
        while self.signal_rx.try_recv().is_ok() {}
        joined
    }
}
