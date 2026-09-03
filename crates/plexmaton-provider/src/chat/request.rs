//! Chat Completions request reconstruction from the semantic record.

use std::collections::BTreeSet;

use plexmaton_agent::{ModelRequest, RequestItem};
use serde_json::{Value, json};

use crate::{
    FunctionTool, ProviderProfile,
    codec::{EncodeError, tool_output},
};

pub(crate) fn encode(
    profile: &ProviderProfile,
    request: &ModelRequest,
    tools: &[FunctionTool],
    max_output_tokens: Option<u32>,
) -> Result<Value, EncodeError> {
    let messages = encode_messages(request)?;
    let tools: Vec<_> = tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.parameters(),
                    "strict": true,
                },
            })
        })
        .collect();
    let mut body = json!({
        "model": profile.model(),
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
        "reasoning_effort": profile.reasoning_effort().as_str(),
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = Value::String("auto".to_owned());
    }
    if let Some(limit) = max_output_tokens {
        body["max_completion_tokens"] = Value::from(limit);
    }
    Ok(body)
}

#[derive(Default)]
struct PendingAssistant {
    reasoning: String,
    content: Option<String>,
    calls: Vec<Value>,
}

impl PendingAssistant {
    fn is_empty(&self) -> bool {
        self.reasoning.is_empty() && self.content.is_none() && self.calls.is_empty()
    }

    fn into_message(self) -> Value {
        let mut message = json!({
            "role": "assistant",
            "content": self.content,
        });
        if !self.reasoning.is_empty() {
            message["reasoning_content"] = Value::String(self.reasoning);
        }
        if !self.calls.is_empty() {
            message["tool_calls"] = Value::Array(self.calls);
        }
        message
    }
}

fn encode_messages(request: &ModelRequest) -> Result<Vec<Value>, EncodeError> {
    let mut messages = Vec::new();
    let mut pending = PendingAssistant::default();
    let mut calls = BTreeSet::new();

    for item in &request.items {
        match item {
            RequestItem::Reasoning { text } => {
                if pending.content.is_some() || !pending.calls.is_empty() {
                    flush_assistant(&mut messages, &mut pending);
                }
                pending.reasoning.push_str(text);
            }
            RequestItem::Assistant { text } => {
                if pending.content.is_some() || !pending.calls.is_empty() {
                    flush_assistant(&mut messages, &mut pending);
                }
                pending.content = Some(text.clone());
            }
            RequestItem::ToolCall(call) => {
                calls.insert(call.call_id.as_str().to_owned());
                pending.calls.push(json!({
                    "id": call.call_id.as_str(),
                    "type": "function",
                    "function": {
                        "name": call.name,
                        "arguments": call.arguments,
                    },
                }));
            }
            RequestItem::User { text } => {
                flush_assistant(&mut messages, &mut pending);
                messages.push(json!({ "role": "user", "content": text }));
            }
            RequestItem::ToolResult { call_id, outcome } => {
                flush_assistant(&mut messages, &mut pending);
                if !calls.contains(call_id.as_str()) {
                    return Err(EncodeError::OrphanToolResult(call_id.to_string()));
                }
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id.as_str(),
                    "content": tool_output(outcome),
                }));
            }
            RequestItem::ProviderReplay(_) => return Err(EncodeError::OpaqueReplayInChat),
        }
    }
    flush_assistant(&mut messages, &mut pending);
    Ok(messages)
}

fn flush_assistant(messages: &mut Vec<Value>, pending: &mut PendingAssistant) {
    if !pending.is_empty() {
        messages.push(std::mem::take(pending).into_message());
    }
}
