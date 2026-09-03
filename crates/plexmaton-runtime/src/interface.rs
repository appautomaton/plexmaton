//! Results and failures crossing the live runtime's public boundary.

use plexmaton_agent::{
    ModelStepId, UndeliveredInput, UndeliveredModelInput, UnresolvedApprovalDecision,
};
use plexmaton_core::{AgentId, ToolCallId};
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
    /// An owned provider operation sent more than one terminal outcome.
    #[error("model step `{0:?}` queued more than one terminal outcome")]
    DuplicateModelTerminal(ModelStepId),
    /// A call tried to start another admission or execution before its owned worker completed.
    #[error("tool call `{0}` already has active runtime work")]
    ToolAlreadyActive(ToolCallId),
    /// A retained tool future completed without the matching owned call and phase.
    #[error("tool call `{0}` completed without a matching runtime owner")]
    UnexpectedToolCompletion(ToolCallId),
    /// The tool future set and its ownership table diverged.
    #[error("one or more owned tool futures disappeared before completion")]
    ToolTaskLost,
    /// The agent requested a model step before every tool future in its batch had joined.
    #[error("a model step started while native tool work was still active")]
    ModelStartedWithToolWork,
    /// A cancelled or terminal provider future panicked instead of settling normally.
    #[error("provider future `{0:?}` terminated unexpectedly")]
    ProviderFutureFailed(ModelStepId),
}
