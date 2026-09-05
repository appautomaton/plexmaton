//! One Messages content block, complete only at content_block_stop.

use plexmaton_agent::{ModelEvent, ProviderReplay, ReplayCompatibility, ToolCall};
use plexmaton_core::ToolCallId;
use serde_json::{Value, json};

use crate::{
    DecodeError, DecodeLimits,
    codec::output_position,
    wire::{object_field, string_field},
};

pub(super) struct Block {
    pub(super) index: usize,
    content: Content,
}

enum Content {
    Text,
    Thinking {
        text: String,
        signature: String,
    },
    Redacted {
        data: String,
    },
    Tool {
        id: String,
        name: String,
        input: ToolInput,
    },
}

enum ToolInput {
    Initial(Value),
    Streaming(String),
}

impl Block {
    pub(super) fn start(
        index: usize,
        value: &Value,
        limits: DecodeLimits,
    ) -> Result<(Self, Vec<ModelEvent>), DecodeError> {
        let mut events = Vec::new();
        let content = match string_field(value, "type")? {
            "text" => {
                let text = string_field(value, "text")?;
                if !text.is_empty() {
                    events.push(ModelEvent::TextDelta {
                        position: output_position(index, 0)?,
                        delta: text.to_owned(),
                    });
                }
                Content::Text
            }
            "thinking" => {
                let text = string_field(value, "thinking")?;
                let signature = match value.get("signature") {
                    None => "",
                    Some(value) => value.as_str().ok_or_else(|| {
                        DecodeError::UnsupportedEvent("thinking_signature".to_owned())
                    })?,
                };
                if signature.len() > limits.max_replay_bytes {
                    return Err(DecodeError::RetainedReplayTooLarge {
                        limit: limits.max_replay_bytes,
                    });
                }
                if !text.is_empty() {
                    events.push(ModelEvent::ReasoningDelta {
                        position: output_position(index, 0)?,
                        delta: text.to_owned(),
                    });
                }
                Content::Thinking {
                    text: text.to_owned(),
                    signature: signature.to_owned(),
                }
            }
            "redacted_thinking" => {
                let data = string_field(value, "data")?;
                if data.is_empty() || data.len() > limits.max_replay_bytes {
                    return Err(DecodeError::RetainedReplayTooLarge {
                        limit: limits.max_replay_bytes,
                    });
                }
                Content::Redacted {
                    data: data.to_owned(),
                }
            }
            "tool_use" => {
                let id = string_field(value, "id")?;
                let name = string_field(value, "name")?;
                for (field, text) in [("id", id), ("name", name)] {
                    if text.is_empty() {
                        return Err(DecodeError::UnsupportedEvent(
                            "empty_tool_identity".to_owned(),
                        ));
                    }
                    if text.len() > limits.max_tool_identity_bytes {
                        return Err(DecodeError::ToolIdentityTooLarge {
                            index,
                            field,
                            limit: limits.max_tool_identity_bytes,
                        });
                    }
                }
                let input = object_field(value, "input")?;
                if input.to_string().len() > limits.max_tool_argument_bytes {
                    return Err(DecodeError::ToolArgumentsTooLarge {
                        index,
                        limit: limits.max_tool_argument_bytes,
                    });
                }
                Content::Tool {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    input: ToolInput::Initial(input.clone()),
                }
            }
            other => {
                return Err(DecodeError::UnsupportedEvent(format!(
                    "content_block:{other}"
                )));
            }
        };
        Ok((Self { index, content }, events))
    }

    pub(super) fn delta(
        &mut self,
        value: &Value,
        limits: DecodeLimits,
    ) -> Result<Vec<ModelEvent>, DecodeError> {
        let position = output_position(self.index, 0)?;
        match (&mut self.content, string_field(value, "type")?) {
            (Content::Text, "text_delta") => Ok(vec![ModelEvent::TextDelta {
                position,
                delta: string_field(value, "text")?.to_owned(),
            }]),
            (Content::Thinking { text, .. }, "thinking_delta") => {
                let delta = string_field(value, "thinking")?;
                if text.len().saturating_add(delta.len()) > limits.max_retained_output_bytes {
                    return Err(DecodeError::RetainedOutputTooLarge {
                        limit: limits.max_retained_output_bytes,
                    });
                }
                text.push_str(delta);
                Ok(vec![ModelEvent::ReasoningDelta {
                    position,
                    delta: delta.to_owned(),
                }])
            }
            (Content::Thinking { signature, .. }, "signature_delta") => {
                let delta = string_field(value, "signature")?;
                if signature.len().saturating_add(delta.len()) > limits.max_replay_bytes {
                    return Err(DecodeError::RetainedReplayTooLarge {
                        limit: limits.max_replay_bytes,
                    });
                }
                signature.push_str(delta);
                Ok(Vec::new())
            }
            (Content::Tool { input, .. }, "input_json_delta") => {
                if let ToolInput::Initial(value) = input {
                    if value.as_object().is_none_or(|value| !value.is_empty()) {
                        return Err(DecodeError::ConflictingToolFragment {
                            index: self.index,
                            field: "input",
                        });
                    }
                    *input = ToolInput::Streaming(String::new());
                }
                let ToolInput::Streaming(arguments) = input else {
                    unreachable!("initial input was converted");
                };
                let delta = string_field(value, "partial_json")?;
                if arguments.len().saturating_add(delta.len()) > limits.max_tool_argument_bytes {
                    return Err(DecodeError::ToolArgumentsTooLarge {
                        index: self.index,
                        limit: limits.max_tool_argument_bytes,
                    });
                }
                arguments.push_str(delta);
                Ok(Vec::new())
            }
            (_, kind) => Err(DecodeError::UnsupportedEvent(format!(
                "content_delta:{kind}"
            ))),
        }
    }

    pub(super) fn finish(
        self,
        compatibility: &ReplayCompatibility,
    ) -> Result<Option<ModelEvent>, DecodeError> {
        let position = output_position(self.index, 0)?;
        let payload = match self.content {
            Content::Text => return Ok(None),
            Content::Thinking { text, signature } => {
                if signature.is_empty() {
                    return Err(DecodeError::UnsupportedEvent(
                        "thinking_without_signature".to_owned(),
                    ));
                }
                json!({"type":"thinking","thinking":text,"signature":signature})
            }
            Content::Redacted { data } => json!({"type":"redacted_thinking","data":data}),
            Content::Tool { id, name, input } => {
                let arguments = match input {
                    ToolInput::Initial(value) => value.to_string(),
                    ToolInput::Streaming(text) => text,
                };
                let value: Value = serde_json::from_str(&arguments)?;
                if !value.is_object() {
                    return Err(DecodeError::UnsupportedEvent(
                        "tool_input_not_object".to_owned(),
                    ));
                }
                let call_id = ToolCallId::new(id).map_err(|_| DecodeError::IncompleteToolCall {
                    index: self.index,
                    field: "id",
                })?;
                return Ok(Some(ModelEvent::Called {
                    position,
                    call: ToolCall {
                        call_id,
                        name,
                        arguments,
                    },
                }));
            }
        };
        let replay = ProviderReplay::new(compatibility.clone(), payload.to_string())
            .map_err(DecodeError::Replay)?;
        Ok(Some(ModelEvent::Replay { position, replay }))
    }
}
