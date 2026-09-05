//! Completed message metadata, kept separately from authoritative semantic text.

use plexmaton_agent::{ModelEvent, ProviderReplay};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::stream::ResponsesDecoder;
use crate::codec::{DecodeError, output_position, retain_bytes};
use crate::wire::string_field;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MessagePhase {
    Commentary,
    FinalAnswer,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ItemStatus {
    InProgress,
    Completed,
    Incomplete,
}

fn item_status(item: &Value) -> Result<Option<ItemStatus>, DecodeError> {
    item.get("status")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value(value.clone()).map_err(DecodeError::Json))
        .transpose()
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ResponseReplay {
    FunctionCall {
        id: Option<String>,
        status: Option<ItemStatus>,
    },
    EmptyMessage {
        id: String,
        phase: Option<MessagePhase>,
        status: Option<ItemStatus>,
    },
    MessagePart {
        id: String,
        phase: Option<MessagePhase>,
        status: Option<ItemStatus>,
        content_index: usize,
        part: MessagePartKind,
        annotations: Vec<Value>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MessagePartKind {
    OutputText,
    Refusal,
}

impl ResponsesDecoder {
    pub(super) fn reasoning_replay(
        &mut self,
        index: usize,
        item: &Value,
    ) -> Result<Vec<ModelEvent>, DecodeError> {
        if item
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_none()
        {
            return Err(DecodeError::UnsupportedEvent(
                "reasoning_without_encrypted_content".to_owned(),
            ));
        }
        let summary = item
            .get("summary")
            .and_then(Value::as_array)
            .ok_or_else(|| DecodeError::UnsupportedEvent("reasoning_without_summary".to_owned()))?
            .iter()
            .map(|part| {
                if part.get("type").and_then(Value::as_str) != Some("summary_text") {
                    return Err(DecodeError::UnsupportedEvent(
                        "unsupported_reasoning_summary_part".to_owned(),
                    ));
                }
                string_field(part, "text")
            })
            .collect::<Result<String, _>>()?;
        let unstreamed = self.reasoning_summary(index)?.finish(&summary, index, 0)?;
        let replay = self.retain_replay(serde_json::to_string(item)?)?;
        let mut events = Vec::with_capacity(2);
        if let Some(summary) = unstreamed {
            retain_bytes(
                &mut self.retained,
                summary.len(),
                self.limits.max_retained_output_bytes,
            )?;
            events.push(ModelEvent::ReasoningDelta {
                position: output_position(index, 0)?,
                delta: summary,
            });
        }
        events.push(ModelEvent::Replay {
            position: output_position(index, 0)?,
            replay,
        });
        Ok(events)
    }

    pub(super) fn message_done(
        &mut self,
        index: usize,
        item: &Value,
    ) -> Result<Vec<ModelEvent>, DecodeError> {
        if string_field(item, "role")? != "assistant" {
            return Err(DecodeError::UnsupportedEvent("message_role".to_owned()));
        }
        let id = string_field(item, "id")?;
        if id.is_empty() {
            return Err(DecodeError::UnsupportedEvent("empty_message_id".to_owned()));
        }
        if id.len() > self.limits.max_tool_identity_bytes {
            return Err(DecodeError::ToolIdentityTooLarge {
                index,
                field: "message_id",
                limit: self.limits.max_tool_identity_bytes,
            });
        }
        let status = item_status(item)?;
        let phase = match item.get("phase").filter(|value| !value.is_null()) {
            Some(value) => Some(serde_json::from_value::<MessagePhase>(value.clone())?),
            None => None,
        };
        let content = item
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| DecodeError::UnsupportedEvent("message_content".to_owned()))?;
        if content.is_empty() {
            let metadata = ResponseReplay::EmptyMessage {
                id: id.to_owned(),
                phase,
                status,
            };
            let replay = self.retain_replay(serde_json::to_string(&metadata)?)?;
            return Ok(vec![ModelEvent::Replay {
                position: output_position(index, 0)?,
                replay,
            }]);
        }
        if content.len() > self.limits.max_output_items {
            return Err(DecodeError::UnsupportedEvent(
                "message_content_length".to_owned(),
            ));
        }
        let mut events = Vec::new();
        for (content_index, part) in content.iter().enumerate() {
            let (kind, text) = match string_field(part, "type")? {
                "output_text" => (MessagePartKind::OutputText, string_field(part, "text")?),
                "refusal" => (MessagePartKind::Refusal, string_field(part, "refusal")?),
                _ => {
                    return Err(DecodeError::UnsupportedEvent(
                        "message_content_type".to_owned(),
                    ));
                }
            };
            if matches!(kind, MessagePartKind::Refusal) {
                self.saw_refusal = true;
            }
            let unstreamed =
                self.text_part(index, content_index)?
                    .confirm(text, index, content_index)?;
            if let Some(delta) = unstreamed {
                retain_bytes(
                    &mut self.retained,
                    delta.len(),
                    self.limits.max_retained_output_bytes,
                )?;
                events.push(ModelEvent::TextDelta {
                    position: output_position(index, content_index)?,
                    delta,
                });
            }
            let annotations = match part.get("annotations") {
                Some(Value::Array(values)) => values.clone(),
                None => Vec::new(),
                _ => {
                    return Err(DecodeError::UnsupportedEvent(
                        "message_annotations".to_owned(),
                    ));
                }
            };
            let metadata = ResponseReplay::MessagePart {
                id: id.to_owned(),
                phase,
                status,
                content_index,
                part: kind,
                annotations,
            };
            let replay = self.retain_replay(serde_json::to_string(&metadata)?)?;
            events.push(ModelEvent::Replay {
                position: output_position(index, content_index)?,
                replay,
            });
        }
        Ok(events)
    }

    pub(super) fn call_replay(
        &mut self,
        index: usize,
        id: Option<String>,
        item: &Value,
    ) -> Result<Option<ModelEvent>, DecodeError> {
        let status = item_status(item)?;
        if id.is_none() && status.is_none() {
            return Ok(None);
        }
        let payload = serde_json::to_string(&ResponseReplay::FunctionCall { id, status })?;
        let replay = self.retain_replay(payload)?;
        Ok(Some(ModelEvent::Replay {
            position: output_position(index, 0)?,
            replay,
        }))
    }

    fn retain_replay(&mut self, payload: String) -> Result<ProviderReplay, DecodeError> {
        if self.replay_items >= self.limits.max_replay_items {
            return Err(DecodeError::TooManyReplayItems {
                limit: self.limits.max_replay_items,
            });
        }
        let next = self
            .replay_bytes
            .checked_add(payload.len())
            .filter(|next| *next <= self.limits.max_replay_bytes)
            .ok_or(DecodeError::RetainedReplayTooLarge {
                limit: self.limits.max_replay_bytes,
            })?;
        let replay = ProviderReplay::new(self.replay_compatibility.clone(), payload)
            .map_err(DecodeError::Replay)?;
        self.replay_bytes = next;
        self.replay_items += 1;
        Ok(replay)
    }
}
