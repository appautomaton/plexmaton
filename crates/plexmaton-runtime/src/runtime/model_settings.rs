//! Idle-boundary model settings. Active turns retain their driver and request environment.

use super::{LiveRuntime, ShutdownState, collaboration::UserControlRefusal};
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
    #[error("This conversation is too long for the selected model; compact it first.")]
    HistoryTooLong,
    #[error("This conversation's delegated context could not be read.")]
    UnresolvedCollaboration,
    #[error("This conversation's history cannot be encoded for the selected model.")]
    IncompatibleHistory,
    #[error("Main controls this delegated Conversation until handoff.")]
    ControlledByMain,
    #[error("Reopen the collaboration before changing this delegated Conversation.")]
    ControlUnavailable,
}

/// An accepted model, and whether reaching it cost the conversation its replay.
///
/// MDL-1: the switch itself is never refused for what the conversation already holds. What it can
/// cost is the earlier replies' native form — PRV-3 carries a finished thought across as text — and
/// the user is told that once, after the fact, because nothing is destroyed and selecting the
/// original model again replays its own history exactly.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelReplacement {
    /// The model the next request uses.
    pub model: ResolvedModel,
    /// Some earlier reply reaches the new model as text rather than as the thought it was.
    pub degraded_history: bool,
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
    ) -> Result<ModelReplacement, ModelChangeRefusal> {
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
        let mut basis = self
            .agent
            .journal()
            .budget_basis(self.agent.selected_head(), &environment)
            .map_err(|_| ModelChangeRefusal::IncompatibleHistory)?;
        if basis.recovery.is_some() {
            return Err(ModelChangeRefusal::HistoryTooLong);
        }
        // MDL-1: this pre-flight has to encode what the next request will encode. A journal
        // projection keeps a delegated turn as a canonical reference (CIN-2), and the request path
        // resolves it to its source before the codec ever sees it. Skipping that step here refused
        // every conversation that had ever delegated — for a reason no choice of model could fix,
        // including choosing the model it was already on.
        self.collaboration_context
            .resolve(&mut basis.request, self.agent.journal())
            .map_err(|_| ModelChangeRefusal::UnresolvedCollaboration)?;
        plexmaton_provider::estimate_request(candidate, &basis.request, tools)
            .map_err(|_| ModelChangeRefusal::IncompatibleHistory)?;
        let degraded_history = plexmaton_provider::degrades_replay(candidate, &basis.request);
        self.driver = driver;
        Ok(ModelReplacement {
            model: selected,
            degraded_history,
        })
    }

    fn validate_model_change(&self, to: &AgentId) -> Result<(), ModelChangeRefusal> {
        if to != &self.agent_id {
            return Err(ModelChangeRefusal::WrongAgent);
        }
        if let Some(refusal) = self.user_control_refusal() {
            return Err(match refusal {
                UserControlRefusal::Main => ModelChangeRefusal::ControlledByMain,
                UserControlRefusal::Unavailable => ModelChangeRefusal::ControlUnavailable,
            });
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
