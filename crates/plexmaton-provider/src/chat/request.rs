//! Chat Completions request reconstruction from the semantic record.

use plexmaton_agent::{
    AssistantBlock, AssistantOutput, ContextAtom, ContextAtomValue, ModelRequest,
};
use serde_json::{Value, json};

use crate::{
    FunctionTool, ResolvedModel,
    codec::{EncodeError, tool_output},
};

pub(crate) fn encode(
    model: &ResolvedModel,
    request: &ModelRequest,
    tools: &[FunctionTool],
    max_output_tokens: Option<u32>,
) -> Result<Value, EncodeError> {
    let messages = encode_messages(model, request)?;
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
        "model": model.wire_id(),
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
        "reasoning_effort": model.reasoning_effort().as_str(),
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

#[derive(Clone, Copy, Default)]
enum AssistantPhase {
    #[default]
    Reasoning,
    Text,
    Calls,
}

impl PendingAssistant {
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

fn encode_messages(
    model: &ResolvedModel,
    request: &ModelRequest,
) -> Result<Vec<Value>, EncodeError> {
    let mut messages = Vec::new();
    for atom in &request.atoms {
        messages.extend(encode_atom(model, atom)?);
    }
    Ok(messages)
}

pub(crate) fn encode_atom(
    model: &ResolvedModel,
    atom: &ContextAtom,
) -> Result<Vec<Value>, EncodeError> {
    let mut messages = Vec::new();
    match atom.value() {
        ContextAtomValue::User { text } => {
            messages.push(json!({ "role": "user", "content": text }));
        }
        ContextAtomValue::Assistant(output) => {
            messages.push(encode_assistant(model, output)?);
        }
        ContextAtomValue::ToolBatch(batch) => {
            messages.push(encode_assistant(model, batch.assistant())?);
            messages.extend(batch.results().iter().map(|result| {
                json!({
                    "role": "tool",
                    "tool_call_id": result.call_id().as_str(),
                    "content": tool_output(result.outcome()),
                })
            }));
        }
    }
    Ok(messages)
}

fn encode_assistant(model: &ResolvedModel, output: &AssistantOutput) -> Result<Value, EncodeError> {
    if let Some(replay) = output.replay() {
        let expected = model.replay_compatibility();
        if replay.compatible_with() != &expected {
            return Err(EncodeError::IncompatibleReplay {
                found: Box::new(replay.compatible_with().clone()),
                expected: Box::new(expected),
            });
        }
        return Err(EncodeError::OpaqueReplayInChat);
    }

    let mut pending = PendingAssistant::default();
    let mut phase = AssistantPhase::default();
    for block in output.blocks() {
        match block {
            AssistantBlock::Reasoning { text, .. } => {
                if !matches!(phase, AssistantPhase::Reasoning) {
                    return Err(EncodeError::UnrepresentableChatOrder);
                }
                pending.reasoning.push_str(text);
            }
            AssistantBlock::Text { text, .. } => {
                if matches!(phase, AssistantPhase::Calls) {
                    return Err(EncodeError::UnrepresentableChatOrder);
                }
                phase = AssistantPhase::Text;
                pending
                    .content
                    .get_or_insert_with(String::new)
                    .push_str(text);
            }
            AssistantBlock::ToolCall { call, .. } => {
                phase = AssistantPhase::Calls;
                pending.calls.push(json!({
                    "id": call.call_id.as_str(),
                    "type": "function",
                    "function": {
                        "name": call.name,
                        "arguments": call.arguments,
                    },
                }));
            }
        }
    }
    Ok(pending.into_message())
}
