//! Messages deltas overwrite cumulative counters; they are never token increments.

use plexmaton_core::{TokenCounts, TokenUsage};
use serde_json::Value;

use crate::codec::{DecodeError, reported_usage};

#[derive(Default)]
pub(super) struct MessagesUsage {
    input: Option<u64>,
    output: Option<u64>,
    cached: u64,
    written: u64,
    reasoning: Option<u64>,
}

impl MessagesUsage {
    pub(super) fn update(&mut self, usage: Option<&Value>) -> Result<TokenUsage, DecodeError> {
        if let Some(usage) = usage.filter(|value| !value.is_null()) {
            if !usage.is_object() {
                return Err(DecodeError::InvalidUsage { field: "usage" });
            }
            if let Some(server) = usage
                .get("server_tool_use")
                .filter(|value| !value.is_null())
            {
                let server = server.as_object().ok_or(DecodeError::InvalidUsage {
                    field: "server_tool_use",
                })?;
                for count in server.values() {
                    let count = count.as_u64().ok_or(DecodeError::InvalidUsage {
                        field: "server_tool_use",
                    })?;
                    if count != 0 {
                        return Err(DecodeError::UnsupportedEvent(
                            "messages_server_tool_usage".into(),
                        ));
                    }
                }
            }
            if let Some(input) = counter(usage, "input_tokens")? {
                self.input = Some(input);
            }
            if let Some(output) = counter(usage, "output_tokens")? {
                self.output = Some(output);
            }
            if let Some(cached) = counter(usage, "cache_read_input_tokens")? {
                self.cached = cached;
            }
            if let Some(written) = counter(usage, "cache_creation_input_tokens")? {
                self.written = written;
            }
            if let Some(details) = usage
                .get("output_tokens_details")
                .filter(|value| !value.is_null())
            {
                if !details.is_object() {
                    return Err(DecodeError::InvalidUsage {
                        field: "output_tokens_details",
                    });
                }
                if let Some(reasoning) = counter(details, "thinking_tokens")? {
                    self.reasoning = Some(reasoning);
                }
            }
        }
        let (Some(input), Some(output)) = (self.input, self.output) else {
            return Ok(TokenUsage::Unavailable);
        };
        // Messages input_tokens is uncached input; omitted cache counters denote no cache usage.
        let input = input
            .checked_add(self.cached)
            .and_then(|n| n.checked_add(self.written))
            .ok_or(DecodeError::InvalidUsage {
                field: "input_tokens",
            })?;
        let total = input.checked_add(output).ok_or(DecodeError::InvalidUsage {
            field: "total_tokens",
        })?;
        reported_usage(TokenCounts {
            input,
            output,
            cached_input: Some(self.cached),
            cache_write_input: Some(self.written),
            reasoning_output: self.reasoning,
            total,
        })
    }
}

fn counter(value: &Value, field: &'static str) -> Result<Option<u64>, DecodeError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or(DecodeError::InvalidUsage { field }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// PRV-5/TIM-3: unsupported hosted-tool charges cannot disappear into a token-only cost.
    #[test]
    fn messages_usage_refuses_unaccounted_server_tools() {
        for server in [
            json!({"web_search_requests":1}),
            json!({"web_fetch_requests":2}),
        ] {
            let usage = json!({"input_tokens":10,"output_tokens":1,"server_tool_use":server});
            assert!(matches!(
                MessagesUsage::default().update(Some(&usage)),
                Err(DecodeError::UnsupportedEvent(_))
            ));
        }
        let usage = json!({"input_tokens":10,"output_tokens":1,"server_tool_use":{"web_search_requests":0,"web_fetch_requests":0}});
        assert!(
            matches!(MessagesUsage::default().update(Some(&usage)), Ok(TokenUsage::Partial(counts)) if counts.input == 10 && counts.output == 1)
        );
        for server in [json!(1), json!({"web_search_requests":"unknown"})] {
            let usage = json!({"input_tokens":10,"output_tokens":1,"server_tool_use":server});
            assert!(matches!(
                MessagesUsage::default().update(Some(&usage)),
                Err(DecodeError::InvalidUsage {
                    field: "server_tool_use"
                })
            ));
        }
    }
}
