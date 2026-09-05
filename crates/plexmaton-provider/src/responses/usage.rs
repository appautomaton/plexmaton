//! Responses terminal usage grammar.

use plexmaton_agent::ModelEvent;
use plexmaton_core::{TokenCounts, TokenUsage};
use serde_json::Value;

use crate::codec::{DecodeError, reported_usage};

pub(super) fn event_usage(event: &Value) -> Result<ModelEvent, DecodeError> {
    let Some(usage) = event
        .get("response")
        .and_then(|response| response.get("usage"))
        .filter(|usage| !usage.is_null())
    else {
        return Ok(ModelEvent::Usage(TokenUsage::Unavailable));
    };
    let input = usage_u64(usage, "input_tokens")?;
    let output = usage_u64(usage, "output_tokens")?;
    let total = usage_u64(usage, "total_tokens")?;
    for field in ["input_tokens_details", "output_tokens_details"] {
        if usage
            .get(field)
            .is_some_and(|details| !details.is_null() && !details.is_object())
        {
            return Err(DecodeError::InvalidUsage { field });
        }
    }
    let input_details = usage.get("input_tokens_details");
    let output_details = usage.get("output_tokens_details");
    let counts = TokenCounts {
        input,
        cached_input: optional_usage_u64(input_details, "cached_tokens")?,
        cache_write_input: optional_usage_u64(input_details, "cache_write_tokens")?,
        output,
        reasoning_output: optional_usage_u64(output_details, "reasoning_tokens")?,
        total,
    };
    Ok(ModelEvent::Usage(reported_usage(counts)?))
}

fn usage_u64(value: &Value, field: &'static str) -> Result<u64, DecodeError> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or(DecodeError::InvalidUsage { field })
}

fn optional_usage_u64(
    details: Option<&Value>,
    field: &'static str,
) -> Result<Option<u64>, DecodeError> {
    let Some(value) = details.and_then(|details| details.get(field)) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .map(Some)
        .ok_or(DecodeError::InvalidUsage { field })
}
