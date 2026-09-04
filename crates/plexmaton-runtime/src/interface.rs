//! Results and failures crossing the live runtime's public boundary.

use plexmaton_agent::{
    ModelStepId, UndeliveredInput, UndeliveredModelInput, UnresolvedApprovalDecision,
};
use plexmaton_core::{AgentId, SessionEventEnvelope, ToolCallId};
use thiserror::Error;

/// File repair performed before a resumed runtime receives the journal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalTailRecovery {
    /// The complete final JSON record lacked only its newline.
    AddedFinalNewline,
    /// An incomplete final fragment was retained in an owner-only sibling.
    IsolatedFinalTail {
        /// Number of exact bytes retained outside the canonical file.
        bytes: u64,
    },
}

/// One startup-only summary of work performed while resuming a session.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionRecovery {
    /// Syntactic file repair, if the final write was incomplete.
    pub tail: Option<JournalTailRecovery>,
    /// Whether canonical recovery settled an unfinished turn without replaying its effects.
    pub interrupted_turn: bool,
}

impl SessionRecovery {
    /// Whether opening required no repair or semantic interruption.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.tail.is_none() && !self.interrupted_turn
    }
}

/// Non-event results retained when an input could not enter the loop boundary it named.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DispatchReport {
    /// User input returned with its exact text and reason.
    pub undelivered: Vec<UndeliveredInput>,
    /// Approval decisions that named no pending request.
    pub unresolved_approvals: Vec<UnresolvedApprovalDecision>,
    /// Provider output refused by model-step correlation.
    pub undelivered_model: Vec<UndeliveredModelInput>,
    /// Durable session failure that prevented accepted input from becoming visible or executable.
    pub persistence_failure: Option<PersistenceFailure>,
    /// Cleanup failures observed while freezing a runtime whose journal can no longer advance.
    pub cleanup_failures: Vec<CleanupFailure>,
}

impl DispatchReport {
    pub(crate) fn is_empty(&self) -> bool {
        self.undelivered.is_empty()
            && self.unresolved_approvals.is_empty()
            && self.undelivered_model.is_empty()
            && self.persistence_failure.is_none()
            && self.cleanup_failures.is_empty()
    }
}

/// One observable result from waiting on the live runtime.
#[derive(Debug, Eq, PartialEq)]
pub enum RuntimeUpdate {
    /// One revisioned semantic event is ready for the projection.
    Event(SessionEventEnvelope),
    /// Non-event ownership or refusal information is ready for the composition root.
    Report(DispatchReport),
    /// Orderly shutdown has no owned work or further output.
    Finished,
}

/// Typed failure of the session durability boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistenceFailure {
    /// The transition wrote no bytes and the exact input can be submitted again after reopen.
    NotWritten,
    /// Some bytes or records may have reached disk; reopen must reconcile before any retry.
    OutcomeUnknown,
}

/// Owned cleanup boundary that failed after the journal had already frozen execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupFailure {
    /// The retained provider future panicked while cancellation joined it.
    Provider,
    /// One or more native tool workers could not be joined cleanly.
    Tools,
    /// The journal writer task could not report a clean exit.
    JournalWriter,
}

/// A live-runtime ownership or routing failure.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Durable construction failed at the existing HTTP/profile boundary.
    #[error(transparent)]
    HttpSetup(#[from] crate::HttpSetupError),
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
    /// The agent refused an attempt audit transition that should match the owned model step.
    #[error("the agent refused a model request-attempt transition: {0:?}")]
    RequestAttemptRefused(plexmaton_agent::RequestAttemptRefusal),
    /// An authorization commit completed without its retained request payload.
    #[error("a model authorization completed without a retained model start")]
    MissingAuthorizedModelStart,
    /// A terminal settlement was requested without an owned model result.
    #[error("a model terminal settlement has no retained active result")]
    MissingActiveModelSettlement,
    /// A terminal model result had no corresponding durable attempt audit.
    #[error("a model terminal result reached delivery before its attempt audit")]
    MissingRequestAttemptAudit,
    /// More than one terminal settlement action was staged for one model owner.
    #[error("a model terminal settlement action is already pending")]
    ModelSettlementAlreadyPending,
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
    /// The bounded journal owner could not accept or finish a command.
    #[error("the session journal writer is unavailable")]
    JournalWriterUnavailable,
    /// A canonical transition could not be appended; this runtime cannot safely continue.
    #[error("the session journal append failed")]
    JournalAppendFailed {
        /// Typed storage boundary failure.
        #[source]
        source: plexmaton_session_store::StoreError,
    },
    /// A prior journal failure made further work unsafe.
    #[error("the session journal requires reopen before more work")]
    JournalRequiresReopen,
    /// A loaded journal could not rebuild the single live agent it names.
    #[error("the session journal cannot rebuild the live agent: {0:?}")]
    JournalProjection(plexmaton_agent::JournalProjectionError),
    /// An event-only caller must collect the pending dispatch report before polling again.
    #[error("the live runtime has a non-event dispatch report ready")]
    DispatchReportPending,
    /// The agent requested a model step before every tool future in its batch had joined.
    #[error("a model step started while native tool work was still active")]
    ModelStartedWithToolWork,
    /// A cancelled or terminal provider future panicked instead of settling normally.
    #[error("provider future `{0:?}` terminated unexpectedly")]
    ProviderFutureFailed(ModelStepId),
    /// The system wall clock cannot be represented by the durable millisecond type.
    #[error("the system wall clock is before the Unix epoch")]
    WallClockBeforeUnixEpoch,
    /// The system wall clock exceeds the durable millisecond representation.
    #[error("the system wall clock exceeds the durable timestamp range")]
    WallClockOutOfRange,
    /// More cancelled submit futures filled the runtime-owned handoff queue.
    #[error("the bounded runtime input handoff queue is full")]
    RuntimeInputQueueFull,
}
