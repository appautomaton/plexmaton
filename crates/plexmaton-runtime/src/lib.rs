//! Owned live execution of one agent over one selected provider transport.
//!
//! The agent decides and this crate performs. It is the only library allowed to own HTTP, model
//! tasks, cancellation and the bounded channel between them; neither provider codecs nor the TUI
//! know it exists (LIVE-1).

mod http;
mod interface;
mod native;
mod runtime;

pub use http::HttpSetupError;
pub use interface::{
    CleanupFailure, CompactionRequest, CompactionRequestRefusal, ConversationRecovery,
    DispatchReport, JournalTailRecovery, PersistenceFailure, RequestedCompactionOutcome,
    RuntimeError, RuntimeUpdate, SkillSummary,
};
pub use native::{NativePermissionCompiler, NativeToolCatalog, NativeToolSetupError};
pub use runtime::{
    ContextBudgetSnapshot, ContextBudgetUnavailable, LiveRuntime, ModelChangeRefusal,
    QueuedBoundary, QueuedInput,
};

pub use runtime::{CodingSessionPermissions, ProjectPermissionConfigurationSource};
