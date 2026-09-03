//! Admission and foreground execution for the native `exec_command` tool.
//!
//! The live runtime registers this executor in its concrete native catalog, but this crate owns no
//! catalog, policy, or runtime state. A caller admits a model request, applies policy, then drives
//! [`CommandTool::execute`] to completion with an owned cancellation token. Partial output,
//! background jobs, PTYs and interactive sessions are outside this boundary (CMD-6).
#![deny(missing_docs)]
#![cfg_attr(not(unix), allow(dead_code))]

#[cfg(not(unix))]
compile_error!("plexmaton-command currently requires Unix process groups");

mod admission;
mod capture;
mod environment;
mod executor;
mod process;
mod result;

pub use admission::{
    COMMAND_DEFINITION_ID, COMMAND_DESCRIPTION, COMMAND_TOOL_NAME, CommandTool,
    CommandToolConfigurationError, DEFAULT_TIMEOUT_MS, MAX_COMMAND_BYTES, MAX_COMMAND_CHARACTERS,
    MAX_TIMEOUT_MS, command_parameters_schema,
};
pub use capture::{CapturedStream, MAX_RETAINED_STREAM_BYTES};
pub use result::{
    CommandExecutionError, CommandOutput, ExitCause, MAX_MODEL_OUTPUT_BYTES, OutputStream,
};
