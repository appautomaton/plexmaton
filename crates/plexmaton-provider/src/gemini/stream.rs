//! Bounded GenerateContent chunks with request-scoped local call identities.

use std::collections::BTreeSet;

use plexmaton_agent::{
    ModelEvent, ProviderReplay, ReplayCompatibility, RequestAttemptId, StopReason, ToolCall,
};
use plexmaton_core::ToolCallId;
use serde_json::Value;

use super::{
    usage::usage,
    wire::{FunctionCall, Part, PartReplay},
};
use crate::{
    DecodeError, DecodeLimits,
    codec::{output_position, retain_bytes},
    environment::identity_key,
};

struct TextPart {
    index: usize,
    thought: bool,
    text_present: bool,
    signature: Option<String>,
}

#[derive(Debug)]
enum Terminal {
    Stopped,
    Failed { code: String },
}

pub(crate) struct GeminiDecoder {
    limits: DecodeLimits,
    compatibility: ReplayCompatibility,
    call_scope: String,
    text: Option<TextPart>,
    next_index: usize,
    upstream_calls: BTreeSet<String>,
    call_count: usize,
    terminal: Option<Terminal>,
    retained: usize,
    replay_bytes: usize,
    replay_items: usize,
}

impl std::fmt::Debug for GeminiDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiDecoder")
            .field("next_index", &self.next_index)
            .field("terminal", &self.terminal)
            .finish_non_exhaustive()
    }
}

impl GeminiDecoder {
    pub(crate) fn new(
        scope: &RequestAttemptId,
        limits: DecodeLimits,
        compatibility: ReplayCompatibility,
    ) -> Self {
        Self {
            limits,
            compatibility,
            call_scope: identity_key(b"plexmaton.gemini_call.v1:", scope.as_str()),
            text: None,
            next_index: 0,
            upstream_calls: BTreeSet::new(),
            call_count: 0,
            terminal: None,
            retained: 0,
            replay_bytes: 0,
            replay_items: 0,
        }
    }

    pub(crate) fn push(&mut self, name: &str, data: &str) -> Result<Vec<ModelEvent>, DecodeError> {
        if name != "message" {
            return Err(DecodeError::UnsupportedEvent(name.to_owned()));
        }
        let chunk: Value = serde_json::from_str(data)?;
        if let Some(error) = chunk.get("error") {
            return Err(DecodeError::ProviderFailed {
                code: error
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
        let mut events = Vec::new();
        if chunk.get("usageMetadata").is_some() {
            events.push(ModelEvent::Usage(usage(&chunk)?));
        }
        let candidates = match chunk.get("candidates") {
            Some(Value::Array(values)) if values.len() <= 1 => values.as_slice(),
            None => &[],
            _ => {
                return Err(DecodeError::UnsupportedEvent(
                    "gemini_candidates".to_owned(),
                ));
            }
        };
        if let Some(candidate) = candidates.first() {
            if candidate
                .get("index")
                .is_some_and(|index| index.as_u64() != Some(0))
            {
                return Err(DecodeError::UnsupportedEvent(
                    "gemini_candidate_index".to_owned(),
                ));
            }
            let finish_reason = candidate.get("finishReason").and_then(Value::as_str);
            if let Some(
                code @ ("MALFORMED_FUNCTION_CALL"
                | "UNEXPECTED_TOOL_CALL"
                | "TOO_MANY_TOOL_CALLS"
                | "MISSING_THOUGHT_SIGNATURE"
                | "MALFORMED_RESPONSE"),
            ) = finish_reason
            {
                if self.terminal.is_some() {
                    return Err(DecodeError::DuplicateFinality);
                }
                // PRV-5/TIM-3: retain this chunk's accounting before reporting declared failure.
                // Its candidate may contain invalid calls; none can enter tool admission.
                self.terminal = Some(Terminal::Failed {
                    code: code.to_owned(),
                });
                self.text = None;
                return Ok(events);
            }
            if let Some(content) = candidate.get("content") {
                if self.terminal.is_some() {
                    return Err(DecodeError::DuplicateFinality);
                }
                if content
                    .get("role")
                    .is_some_and(|role| role.as_str() != Some("model"))
                {
                    return Err(DecodeError::UnsupportedEvent(
                        "gemini_content_role".to_owned(),
                    ));
                }
                let parts = content
                    .get("parts")
                    .and_then(Value::as_array)
                    .ok_or_else(|| DecodeError::UnsupportedEvent("gemini_parts".to_owned()))?;
                if parts.len() > self.limits.max_output_items {
                    return Err(DecodeError::TooManyOutputItems {
                        limit: self.limits.max_output_items,
                    });
                }
                for (offset, part) in parts.iter().enumerate() {
                    if offset > 0 {
                        self.text = None;
                    }
                    let part: Part = serde_json::from_value(part.clone())?;
                    events.extend(self.part(part)?);
                }
            }
            if let Some(reason) =
                finish_reason.filter(|reason| *reason != "FINISH_REASON_UNSPECIFIED")
            {
                let reason = match reason {
                    "STOP" if self.call_count > 0 => StopReason::ToolCalls,
                    "STOP" => StopReason::EndOfTurn,
                    "MAX_TOKENS" => StopReason::OutputLimit,
                    "SAFETY"
                    | "RECITATION"
                    | "BLOCKLIST"
                    | "PROHIBITED_CONTENT"
                    | "SPII"
                    | "ESCALATION"
                    | "PUP_LIMITED_DISABLED" => StopReason::Refused,
                    "OTHER" | "LANGUAGE" => StopReason::Unspecified,
                    other => return Err(DecodeError::UnknownStopReason(other.to_owned())),
                };
                if self.call_count > 0 && reason != StopReason::ToolCalls {
                    return Err(DecodeError::IncompleteToolCall {
                        index: self.next_index,
                        field: "finishReason",
                    });
                }
                self.complete(reason, &mut events)?;
            }
        } else if let Some(reason) = chunk
            .get("promptFeedback")
            .and_then(|value| value.get("blockReason"))
            .and_then(Value::as_str)
            && reason != "BLOCK_REASON_UNSPECIFIED"
        {
            self.complete(StopReason::Refused, &mut events)?;
        }
        Ok(events)
    }

    fn complete(
        &mut self,
        reason: StopReason,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), DecodeError> {
        if self.terminal.replace(Terminal::Stopped).is_some() {
            return Err(DecodeError::DuplicateFinality);
        }
        self.text = None;
        events.push(ModelEvent::Stopped(reason));
        Ok(())
    }

    fn part(&mut self, part: Part) -> Result<Vec<ModelEvent>, DecodeError> {
        crate::wire::check_additive_fields(&part.extra, "gemini_part")?;
        if let Some(call) = part.function_call {
            if part.text.is_some() || part.thought == Some(true) {
                return Err(DecodeError::UnsupportedEvent(
                    "mixed_gemini_part".to_owned(),
                ));
            }
            self.text = None;
            return self.function_call(call, part.thought_signature);
        }
        if part.text.is_none() && part.thought_signature.is_none() {
            if !part.extra.is_empty() && part.thought.is_none() {
                return Ok(Vec::new());
            }
            return Err(DecodeError::UnsupportedEvent(
                "empty_gemini_part".to_owned(),
            ));
        }
        let thought = part.thought.unwrap_or(false);
        let signature = part.thought_signature.filter(|value| !value.is_empty());
        if let Some(text) = &self.text
            && (text.thought != thought
                || text
                    .signature
                    .as_ref()
                    .zip(signature.as_ref())
                    .is_some_and(|(a, b)| a != b))
        {
            self.text = None;
        }
        if self.text.is_none() {
            let index = self.reserve_part()?;
            self.text = Some(TextPart {
                index,
                thought,
                text_present: false,
                signature: None,
            });
        }
        let current = self
            .text
            .as_mut()
            .unwrap_or_else(|| unreachable!("text part was opened"));
        current.text_present |= part.text.is_some();
        let index = current.index;
        let text_present = current.text_present;
        let new_signature = current.signature.is_none();
        let position = output_position(index, 0)?;
        let mut events = Vec::new();
        if let Some(delta) = part.text.filter(|text| !text.is_empty()) {
            retain_bytes(
                &mut self.retained,
                delta.len(),
                self.limits.max_retained_output_bytes,
            )?;
            events.push(if thought {
                ModelEvent::ReasoningDelta { position, delta }
            } else {
                ModelEvent::TextDelta { position, delta }
            });
        }
        if let Some(signature) = signature
            && new_signature
        {
            let metadata = PartReplay::Text {
                thought,
                text_present,
                signature: signature.clone(),
            };
            events.push(self.replay(index, &metadata)?);
            self.text
                .as_mut()
                .unwrap_or_else(|| unreachable!("text part remains open"))
                .signature = Some(signature);
        }
        Ok(events)
    }

    fn function_call(
        &mut self,
        mut call: FunctionCall,
        signature: Option<String>,
    ) -> Result<Vec<ModelEvent>, DecodeError> {
        call.id = call.id.filter(|id| !id.is_empty());
        if self.call_count >= self.limits.max_tool_calls {
            return Err(DecodeError::TooManyToolCalls {
                limit: self.limits.max_tool_calls,
            });
        }
        let index = self.reserve_part()?;
        if call.name.is_empty() {
            return Err(DecodeError::IncompleteToolCall {
                index,
                field: "name",
            });
        }
        for (field, value) in [
            ("name", Some(call.name.as_str())),
            ("id", call.id.as_deref()),
        ] {
            if value.is_some_and(|text| text.len() > self.limits.max_tool_identity_bytes) {
                return Err(DecodeError::ToolIdentityTooLarge {
                    index,
                    field,
                    limit: self.limits.max_tool_identity_bytes,
                });
            }
        }
        if let Some(id) = &call.id
            && !self.upstream_calls.insert(id.clone())
        {
            return Err(DecodeError::ConflictingToolFragment {
                index,
                field: "upstream_call_id",
            });
        }
        let args = call.args.unwrap_or_else(|| serde_json::json!({}));
        if !args.is_object() {
            return Err(DecodeError::UnsupportedEvent(
                "gemini_arguments_not_object".to_owned(),
            ));
        }
        let arguments = args.to_string();
        if arguments.len() > self.limits.max_tool_argument_bytes {
            return Err(DecodeError::ToolArgumentsTooLarge {
                index,
                limit: self.limits.max_tool_argument_bytes,
            });
        }
        let call_id =
            ToolCallId::new(format!("gemini-{}-{index}", self.call_scope)).map_err(|_| {
                DecodeError::IncompleteToolCall {
                    index,
                    field: "local_call_id",
                }
            })?;
        let replay = self.replay(
            index,
            &PartReplay::FunctionCall {
                upstream_id: call.id,
                signature: signature.filter(|value| !value.is_empty()),
            },
        )?;
        self.call_count += 1;
        Ok(vec![
            ModelEvent::Called {
                position: output_position(index, 0)?,
                call: ToolCall {
                    call_id,
                    name: call.name,
                    arguments,
                },
            },
            replay,
        ])
    }

    fn reserve_part(&mut self) -> Result<usize, DecodeError> {
        if self.next_index >= self.limits.max_output_items {
            return Err(DecodeError::TooManyOutputItems {
                limit: self.limits.max_output_items,
            });
        }
        let index = self.next_index;
        self.next_index += 1;
        Ok(index)
    }

    fn replay(&mut self, index: usize, metadata: &PartReplay) -> Result<ModelEvent, DecodeError> {
        if self.replay_items >= self.limits.max_replay_items {
            return Err(DecodeError::TooManyReplayItems {
                limit: self.limits.max_replay_items,
            });
        }
        let payload = serde_json::to_string(metadata)?;
        self.replay_bytes = self
            .replay_bytes
            .checked_add(payload.len())
            .filter(|bytes| *bytes <= self.limits.max_replay_bytes)
            .ok_or(DecodeError::RetainedReplayTooLarge {
                limit: self.limits.max_replay_bytes,
            })?;
        self.replay_items += 1;
        let replay = ProviderReplay::new(self.compatibility.clone(), payload)
            .map_err(DecodeError::Replay)?;
        Ok(ModelEvent::Replay {
            position: output_position(index, 0)?,
            replay,
        })
    }

    pub(crate) fn finish(self) -> Result<(), DecodeError> {
        match self.terminal {
            None => Err(DecodeError::IncompleteStream),
            Some(Terminal::Stopped) => Ok(()),
            Some(Terminal::Failed { code }) => {
                Err(DecodeError::ProviderFailed { code: Some(code) })
            }
        }
    }
}
