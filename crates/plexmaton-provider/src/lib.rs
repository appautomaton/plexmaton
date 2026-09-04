//! OpenAI-compatible wire codecs for Plexmaton's semantic model boundary.
//!
//! This crate owns configuration, request encoding and streaming decode. It owns no agent loop,
//! tool executor, approval state, transcript projection, HTTP task or terminal.

mod budget;
mod chat;
mod codec;
mod config;
mod cost;
mod environment;
mod responses;
mod sse;

pub use budget::{ContextBudgetError, budget_ledger};
pub use codec::{
    DecodeError, DecodeLimits, EncodeError, FunctionTool, FunctionToolError, OpenAiCodec,
    classify_http_error, encode_request,
};
pub use config::{
    ApiKey, ConfigError, ModelApi, ModelCost, ModelRegistry, ModelSelection, ReasoningEffort,
    ResolvedModel, TokenEstimator, resolve_api_key, resolve_home,
};
pub use cost::request_cost;
pub use environment::request_environment;
pub use sse::{SseDecodeError, drive_sse};
