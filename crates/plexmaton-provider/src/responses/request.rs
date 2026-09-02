//! Stateless Responses request reconstruction.

use std::collections::BTreeSet;

use plexmaton_agent::{ModelRequest, RequestItem};
use serde_json::{Value, json};

use crate::{
    FunctionTool, ProviderProfile,
    codec::{EncodeError, RESPONSES_CODEC_ID, tool_output},
};

pub(crate) fn encode(
    profile: &ProviderProfile,
    request: &ModelRequest,
    tools: &[FunctionTool],
    max_output_tokens: Option<u32>,
) -> Result<Value, EncodeError> {
    let input = encode_input(request)?;
    let tools: Vec<_> = tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name(),
                "description": tool.description(),
                "parameters": tool.parameters(),
                "strict": true,
            })
        })
        .collect();
    let mut body = json!({
        "model": profile.model(),
        "input": input,
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
        "reasoning": { "effort": profile.reasoning_effort().as_str() },
        "parallel_tool_calls": true,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = Value::String("auto".to_owned());
    }
    if let Some(limit) = max_output_tokens {
        body["max_output_tokens"] = Value::from(limit);
    }
    Ok(body)
}

fn encode_input(request: &ModelRequest) -> Result<Vec<Value>, EncodeError> {
    let mut input = Vec::new();
    let mut calls = BTreeSet::new();
    for item in &request.items {
        match item {
            RequestItem::User { text } => {
                input.push(json!({ "role": "user", "content": text }));
            }
            RequestItem::Assistant { text } => {
                input.push(json!({
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": text }],
                }));
            }
            RequestItem::Reasoning { .. } => {
                return Err(EncodeError::PlainReasoningInResponses);
            }
            RequestItem::ProviderReplay(replay) => {
                if replay.codec().as_str() != RESPONSES_CODEC_ID {
                    return Err(EncodeError::WrongReplayCodec {
                        found: replay.codec().as_str().to_owned(),
                        expected: RESPONSES_CODEC_ID,
                    });
                }
                let item: Value = serde_json::from_str(replay.payload())
                    .map_err(EncodeError::InvalidReplayJson)?;
                if item.get("type").and_then(Value::as_str) != Some("reasoning")
                    || item
                        .get("encrypted_content")
                        .and_then(Value::as_str)
                        .is_none()
                {
                    return Err(EncodeError::InvalidReplayItem);
                }
                input.push(item);
            }
            RequestItem::ToolCall(call) => {
                calls.insert(call.call_id.as_str().to_owned());
                input.push(json!({
                    "type": "function_call",
                    "call_id": call.call_id.as_str(),
                    "name": call.name,
                    "arguments": call.arguments,
                }));
            }
            RequestItem::ToolResult { call_id, outcome } => {
                if !calls.contains(call_id.as_str()) {
                    return Err(EncodeError::OrphanToolResult(call_id.to_string()));
                }
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id.as_str(),
                    "output": tool_output(outcome),
                }));
            }
        }
    }
    Ok(input)
}
