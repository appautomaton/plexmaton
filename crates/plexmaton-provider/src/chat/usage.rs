//! Chat Completions usage grammar.

use plexmaton_core::{TokenCounts, TokenUsage};
use serde::Deserialize;

use crate::codec::{DecodeError, reported_usage};

#[derive(Deserialize)]
pub(super) struct ChatUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    prompt_tokens_details: Option<ChatInputDetails>,
    completion_tokens_details: Option<ChatOutputDetails>,
}

impl ChatUsage {
    pub(super) fn into_semantic(self) -> Result<TokenUsage, DecodeError> {
        reported_usage(TokenCounts {
            input: self.prompt_tokens,
            cached_input: self
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens),
            cache_write_input: self
                .prompt_tokens_details
                .and_then(|details| details.cache_write_tokens),
            output: self.completion_tokens,
            reasoning_output: self
                .completion_tokens_details
                .and_then(|details| details.reasoning_tokens),
            total: self.total_tokens,
        })
    }
}

#[derive(Deserialize)]
struct ChatInputDetails {
    cached_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct ChatOutputDetails {
    reasoning_tokens: Option<u64>,
}
