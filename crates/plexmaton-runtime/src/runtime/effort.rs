//! Idle-boundary effort replacement. Active turns retain their driver and request environment.

use super::{LiveRuntime, ShutdownState};
use plexmaton_core::{AgentId, ReasoningEffort};
use plexmaton_provider::ResolvedModel;

/// Why the addressed conversation cannot accept a different effort now.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EffortChangeRefusal {
    #[error("This effort command belongs to another conversation.")]
    WrongAgent,
    #[error("Wait for this conversation's current work to finish before changing effort.")]
    Busy,
    #[error("This conversation is shutting down.")]
    ShuttingDown,
    #[error("Reopen this conversation before changing effort after a persistence failure.")]
    PersistenceFailed,
    #[error("This model has no editable effort configuration.")]
    Unavailable,
    #[error("This effort is not allowed by the configured model.")]
    Unsupported,
}

impl LiveRuntime {
    /// The exact model used by the next request; status and the TUI read this owner.
    #[must_use]
    pub fn configured_model(&self) -> Option<&ResolvedModel> {
        self.driver.budget_inputs().map(|(model, _)| model)
    }

    /// Atomically replace an idle model driver; no task, journal effect or network work is started.
    pub fn set_reasoning_effort(
        &mut self,
        to: &AgentId,
        effort: ReasoningEffort,
    ) -> Result<ResolvedModel, EffortChangeRefusal> {
        if to != &self.agent_id {
            return Err(EffortChangeRefusal::WrongAgent);
        }
        if self.shutdown_state != ShutdownState::Open {
            return Err(EffortChangeRefusal::ShuttingDown);
        }
        if self.journal_failed {
            return Err(EffortChangeRefusal::PersistenceFailed);
        }
        if self.agent.is_running()
            || self.agent.queued_for_next_turn().next().is_some()
            || self.agent.queued_for_next_step().next().is_some()
            || self.agent.pending_approvals().next().is_some()
            || self.has_active_work()
        {
            return Err(EffortChangeRefusal::Busy);
        }
        let driver = self.driver.with_reasoning_effort(effort)?;
        let model = driver
            .budget_inputs()
            .ok_or(EffortChangeRefusal::Unavailable)?
            .0
            .clone();
        self.driver = driver;
        Ok(model)
    }
}
