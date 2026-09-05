//! Gemini prompt includes cache reads; generated output includes candidates and thoughts.

use plexmaton_core::{TokenCounts, TokenUsage};
use serde_json::Value;

use crate::{DecodeError, codec::reported_usage};

pub(super) fn usage(value: &Value) -> Result<TokenUsage, DecodeError> {
    let Some(value) = value.get("usageMetadata").filter(|value| !value.is_null()) else {
        return Ok(TokenUsage::Unavailable);
    };
    let object = value.as_object().ok_or(DecodeError::InvalidUsage {
        field: "usageMetadata",
    })?;
    if object.is_empty() {
        return Ok(TokenUsage::Unavailable);
    }
    if count(value, "toolUsePromptTokenCount")? != 0 {
        return Err(DecodeError::UnsupportedEvent(
            "gemini_server_tool_usage".to_owned(),
        ));
    }
    let input = count(value, "promptTokenCount")?;
    let reasoning = count(value, "thoughtsTokenCount")?;
    let output = count(value, "candidatesTokenCount")?
        .checked_add(reasoning)
        .ok_or(DecodeError::InvalidUsage {
            field: "candidatesTokenCount",
        })?;
    let total = input.checked_add(output).ok_or(DecodeError::InvalidUsage {
        field: "totalTokenCount",
    })?;
    if value.get("totalTokenCount").is_some() && count(value, "totalTokenCount")? != total {
        return Err(DecodeError::InvalidUsage {
            field: "totalTokenCount",
        });
    }
    reported_usage(TokenCounts {
        input,
        output,
        total,
        cached_input: Some(count(value, "cachedContentTokenCount")?),
        cache_write_input: Some(0),
        reasoning_output: Some(reasoning),
    })
}

fn count(value: &Value, field: &'static str) -> Result<u64, DecodeError> {
    match value.get(field) {
        // GenerateContent's protobuf JSON omits counters with their zero value.
        None => Ok(0),
        Some(value) => value.as_u64().ok_or(DecodeError::InvalidUsage { field }),
    }
}
