//! Results and failures crossing the live runtime's public boundary.

use plexmaton_agent::{
    ModelStepId, UndeliveredInput, UndeliveredModelInput, UnresolvedApprovalDecision,
};
use plexmaton_core::AgentId;
use thiserror::Error;

/// Non-event results retained when an input could not enter the loop boundary it named.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DispatchReport {
    /// User input returned with its exact text and reason.
    pub undelivered: Vec<UndeliveredInput>,
    /// Approval decisions that named no pending request.
    pub unresolved_approvals: Vec<UnresolvedApprovalDecision>,
    /// Provider output refused by model-step correlation.
    pub undelivered_model: Vec<UndeliveredModelInput>,
}

/// A live-runtime ownership or routing failure.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Input named an agent this runtime does not own.
    #[error("runtime owns `{expected}`, not `{received}`")]
    WrongAgent {
        /// Sole agent accepted by this runtime.
        expected: AgentId,
        /// Address supplied by the caller.
        received: AgentId,
    },
    /// Shutdown has begun and no new work may enter.
    #[error("the live runtime is shutting down")]
    ShuttingDown,
    /// The loop requested a second concurrent model operation.
    #[error("model step `{requested:?}` started while `{active:?}` was still owned")]
    ModelAlreadyActive {
        /// Currently owned operation.
        active: ModelStepId,
        /// New operation the loop requested.
        requested: ModelStepId,
    },
    /// An owned provider task sent more than one terminal outcome.
    #[error("model step `{0:?}` queued more than one terminal outcome")]
    DuplicateModelTerminal(ModelStepId),
    /// Slice 6 advertises no tools, so no run effect can be valid yet.
    #[error("the live text runtime received a tool run effect")]
    UnexpectedToolRun,
    /// A cancelled or terminal provider task panicked instead of joining normally.
    #[error("provider task `{0:?}` terminated unexpectedly")]
    ProviderTaskFailed(ModelStepId),
}
