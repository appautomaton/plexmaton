//! Owned live execution of one agent over one selected provider transport.
//!
//! The agent decides and this crate performs. It is the only library allowed to own HTTP, model
//! tasks, cancellation and the bounded channel between them; neither provider codecs nor the TUI
//! know it exists (LIVE-1).

mod http;
mod interface;
mod runtime;

pub use http::HttpSetupError;
pub use interface::{DispatchReport, RuntimeError};
pub use runtime::LiveRuntime;
