//! Owned live execution of one agent over one selected provider transport.
//!
//! The agent decides and this crate performs. It is the only library allowed to own HTTP, model
//! tasks, cancellation and the bounded channel between them; neither provider codecs nor the TUI
//! know it exists (LIVE-1).

mod collaboration_ingress;
mod collaboration_read;
mod collaboration_tools;
mod collaboration_writer;
mod delegated_factory;
mod http;
mod interface;
mod native;
mod owned_collaboration;
mod owned_runner;
mod runtime;

pub use collaboration_ingress::{
    ChildCollaborationIngress, CollaborationArtifactRegistrationError, CollaborationArtifactSource,
    CollaborationIngressFailure, CollaborationIngressOutcome, CollaborationIngressRefusal,
    CollaborationIngressSettlement, CollaborationSessionSource, MainCollaborationIngress,
    MainRuntimeIdentity, OwnedCollaborationActivity, RegisteredCollaborationTarget,
};
pub use collaboration_read::{
    CollaborationReadError, CollaborationSessionMailProjection, SessionMailInclusion,
    SessionProjectedMail,
};
pub use collaboration_tools::{
    ArtifactSelector, ChildMailIntent, CollaborationToolArgumentError, CollaborationToolRequest,
    CollaborationToolScope, DELEGATE_DEFINITION_ID, DELEGATE_TOOL_NAME, DelegateIntent,
    HANDOFF_DEFINITION_ID, HANDOFF_TOOL_NAME, HandoffIntent, MainMailIntent,
    SEND_MAIL_CHILD_DEFINITION_ID, SEND_MAIL_MAIN_DEFINITION_ID, SEND_MAIL_TOOL_NAME,
    TargetSelector, UPDATE_TASK_DEFINITION_ID, UPDATE_TASK_TOOL_NAME, UpdateTaskIntent,
    collaboration_tool_definitions, parse_collaboration_tool,
};
pub(crate) use collaboration_writer::PreparedChildExecution;
pub use collaboration_writer::{
    CollaborationWriter, CollaborationWriterError, ScheduledTurnRequest,
};
pub use delegated_factory::{DelegatedChildFactory, DelegatedChildFactoryError};
pub use http::HttpSetupError;
pub use interface::{
    CleanupFailure, CompactionRequest, CompactionRequestRefusal, ConversationRecovery,
    DelegatedRuntimeBinding, DispatchReport, JournalTailRecovery, PersistenceFailure,
    RequestedCompactionOutcome, RuntimeError, RuntimeUpdate, SkillSummary, TreeAdmission,
    TreeRequestRefusal,
};
pub use native::{NativePermissionCompiler, NativeToolCatalog, NativeToolSetupError};
pub use owned_collaboration::{
    MAX_OWNED_RUNNERS, OwnedCollaboration, OwnedHandoffFailure, OwnedHandoffReport,
    OwnedScheduleFailure, OwnedSchedulingError, OwnedShutdownFailure, OwnedShutdownReport,
    OwnedShutdownSettlement, OwnedStopReport, RunnerRegistrationError, RunnerRegistrationReason,
    SchedulerLimits, WakeAdmission, WakeFailure, WakeRefusal,
};
pub(crate) use owned_runner::{ChildStartError, OwnedChildRunner};
pub use owned_runner::{
    OwnedRunnerError, OwnedRunnerUpdate, RunnerGeneration, RunnerIdentity, WakeHint,
};
pub use runtime::{
    ContextBudgetSnapshot, ContextBudgetUnavailable, LiveRuntime, ModelChangeRefusal,
    QueuedBoundary, QueuedInput,
};

pub use runtime::{CodingSessionPermissions, ProjectPermissionConfigurationSource};
