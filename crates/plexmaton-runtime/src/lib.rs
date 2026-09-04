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
    CleanupFailure, DispatchReport, JournalTailRecovery, PersistenceFailure, RuntimeError,
    RuntimeUpdate, SessionRecovery,
};
pub use native::{MAX_NATIVE_TOOL_RESULT_BYTES, NativeToolCatalog, NativeToolSetupError};
pub use runtime::LiveRuntime;
