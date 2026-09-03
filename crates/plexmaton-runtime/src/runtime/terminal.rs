//! Terminal provider outcomes held until their exact task has joined.

use plexmaton_agent::{Input, ModelError, ModelEvent, ModelStepId};

use super::LiveRuntime;
use crate::RuntimeError;

pub(super) enum QueuedTerminal {
    Streamed(ModelEvent),
    Failed(ModelError),
}

impl LiveRuntime {
    pub(super) fn queue_terminal(
        &mut self,
        step_id: ModelStepId,
        terminal: QueuedTerminal,
    ) -> Result<(), RuntimeError> {
        self.supply_missing_usage()?;
        let active = self
            .active
            .as_mut()
            .unwrap_or_else(|| unreachable!("the matching active task still exists"));
        if active.terminal.is_some() {
            return Err(RuntimeError::DuplicateModelTerminal(step_id));
        }
        active.terminal = Some(terminal);
        Ok(())
    }

    pub(super) fn deliver_terminal(
        &mut self,
        step_id: ModelStepId,
        terminal: QueuedTerminal,
    ) -> Result<(), RuntimeError> {
        let reaction = match terminal {
            QueuedTerminal::Streamed(event) => {
                self.agent.handle(Input::Streamed { step_id, event })
            }
            QueuedTerminal::Failed(error) => self.agent.handle(Input::Failed { step_id, error }),
        };
        self.apply_reaction(reaction)
    }

    pub(super) async fn cancel_active(&mut self) -> Result<(), RuntimeError> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        active.cancellation.cancel();
        let step_id = active.step_id.clone();
        let joined = (&mut active.task)
            .await
            .map_err(|_| RuntimeError::ProviderTaskFailed(step_id.clone()));
        let active = self
            .active
            .take()
            .unwrap_or_else(|| unreachable!("the awaited active task is still owned"));
        joined?;
        if let Some(terminal) = active.terminal {
            self.deliver_terminal(step_id, terminal)?;
        }
        Ok(())
    }
}
