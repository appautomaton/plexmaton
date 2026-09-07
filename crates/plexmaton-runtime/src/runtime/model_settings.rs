//! Idle-boundary model settings. Active turns retain their driver and request environment.

use super::{LiveRuntime, ShutdownState};
use plexmaton_core::{AgentId, ReasoningEffort};
use plexmaton_provider::{ApiKey, ResolvedModel};

/// Why the addressed conversation cannot replace its model settings now.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ModelChangeRefusal {
    #[error("This settings command belongs to another conversation.")]
    WrongAgent,
    #[error("Wait for this conversation's current work to finish before changing these settings.")]
    Busy,
    #[error("This conversation is shutting down.")]
    ShuttingDown,
    #[error("Reopen this conversation before changing these settings after a persistence failure.")]
    PersistenceFailed,
    #[error("This conversation has no editable model settings.")]
    Unavailable,
    #[error("This effort is not allowed by the configured model.")]
    Unsupported,
    #[error("The selected provider credential is not excluded from command execution.")]
    UnprotectedCredential,
    #[error("The selected model could not be configured.")]
    InvalidModel,
    #[error("This conversation contains history the selected model cannot replay.")]
    IncompatibleHistory,
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
    ) -> Result<ResolvedModel, ModelChangeRefusal> {
        self.validate_model_change(to)?;
        let driver = self.driver.with_reasoning_effort(effort)?;
        let model = driver
            .budget_inputs()
            .ok_or(ModelChangeRefusal::Unavailable)?
            .0
            .clone();
        self.driver = driver;
        Ok(model)
    }
    /// Select a configured model at the same idle boundary as effort. Workspace guidance and
    /// historical request records are retained; the replacement starts no network or journal work.
    pub fn set_model(
        &mut self,
        to: &AgentId,
        model: ResolvedModel,
        key: ApiKey,
    ) -> Result<ResolvedModel, ModelChangeRefusal> {
        self.validate_model_change(to)?;
        if !self.tools.excludes_api_key_environment(model.api_key_env()) {
            return Err(ModelChangeRefusal::UnprotectedCredential);
        }
        let current = self
            .configured_model()
            .ok_or(ModelChangeRefusal::Unavailable)?;
        let model = model
            .with_workspace_instructions(current.workspace_instructions().to_owned())
            .map_err(|_| ModelChangeRefusal::InvalidModel)?;
        let driver = self.driver.with_model(model, key)?;
        let selected = driver
            .budget_inputs()
            .ok_or(ModelChangeRefusal::Unavailable)?
            .0
            .clone();
        let (candidate, tools) = driver
            .budget_inputs()
            .ok_or(ModelChangeRefusal::Unavailable)?;
        let environment = plexmaton_provider::request_environment(
            candidate,
            tools,
            Some(candidate.max_output_tokens()),
        );
        let basis = self
            .agent
            .journal()
            .budget_basis(self.agent.selected_head(), &environment)
            .map_err(|_| ModelChangeRefusal::IncompatibleHistory)?;
        if basis.recovery.is_some() {
            return Err(ModelChangeRefusal::IncompatibleHistory);
        }
        plexmaton_provider::estimate_request(candidate, &basis.request, tools)
            .map_err(|_| ModelChangeRefusal::IncompatibleHistory)?;
        self.driver = driver;
        Ok(selected)
    }

    fn validate_model_change(&self, to: &AgentId) -> Result<(), ModelChangeRefusal> {
        if to != &self.agent_id {
            return Err(ModelChangeRefusal::WrongAgent);
        }
        if self.shutdown_state != ShutdownState::Open {
            return Err(ModelChangeRefusal::ShuttingDown);
        }
        if self.journal_failed {
            return Err(ModelChangeRefusal::PersistenceFailed);
        }
        if self.agent.is_running()
            || self.agent.queued_for_next_turn().next().is_some()
            || self.agent.queued_for_next_step().next().is_some()
            || self.agent.pending_approvals().next().is_some()
            || self.has_active_work()
        {
            return Err(ModelChangeRefusal::Busy);
        }
        Ok(())
    }
}
