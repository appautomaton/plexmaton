//! Provider wire codecs for Plexmaton's semantic model boundary.
//!
//! This crate owns configuration, request encoding and streaming decode. It owns no agent loop,
//! tool executor, approval state, transcript projection, HTTP task or terminal.

mod budget;
mod chat;
mod codec;
mod compaction;
mod config;
mod cost;
mod environment;
mod gemini;
mod messages;
mod responses;
mod sse;
mod wire;

pub use budget::{
    BudgetedContext, ContextBudgetError, budget_ledger, budgeted_context, estimate_request,
};
pub use codec::{
    DecodeError, DecodeLimits, EncodeError, FunctionTool, FunctionToolError, ProviderCodec,
    classify_http_error, encode_request,
};
pub use compaction::{
    CompactionInput, CompactionPreparationError, PreparedCompaction, ReplacementFit,
    plan_compaction, validate_compaction_output, validate_replacement,
};
pub use config::{
    ApiKey, ConfigError, ModelApi, ModelCost, ModelRegistry, ModelSelection, PromptCache,
    ReasoningEffort, ResolvedModel, TokenEstimator, resolve_api_key, resolve_home,
};
pub use cost::request_cost;
pub use environment::request_environment;
pub use sse::{SseDecodeError, drive_sse};
