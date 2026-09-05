//! Chat Completions request and stream grammar.

use std::collections::{BTreeMap, BTreeSet};

use plexmaton_agent::{ModelEvent, ProviderReplay, ReplayCompatibility, StopReason, ToolCall};
use plexmaton_core::{TokenUsage, ToolCallId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codec::{DecodeError, DecodeLimits, output_position, retain_bytes};

mod request;
mod usage;

pub(crate) use request::{encode, encode_atom};
use usage::ChatUsage;

#[derive(Debug)]
pub(crate) struct ChatDecoder {
    limits: DecodeLimits,
    retained: usize,
    calls: BTreeMap<usize, CallAssembly>,
    completed_call_ids: BTreeSet<ToolCallId>,
    saw_refusal: bool,
    stopped: bool,
    done: bool,
    usage_reported: bool,
    reasoning_field: Option<ReasoningField>,
    compatibility: ReplayCompatibility,
}

#[derive(Debug, Default)]
struct CallAssembly {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ChatDecoder {
    pub(crate) fn new(limits: DecodeLimits, compatibility: ReplayCompatibility) -> Self {
        Self {
            limits,
            retained: 0,
            calls: BTreeMap::new(),
            completed_call_ids: BTreeSet::new(),
            saw_refusal: false,
            stopped: false,
            done: false,
            usage_reported: false,
            reasoning_field: None,
            compatibility,
        }
    }

    pub(crate) fn push(
        &mut self,
        event_name: &str,
        data: &str,
    ) -> Result<Vec<ModelEvent>, DecodeError> {
        if event_name != "message" {
            return Err(DecodeError::UnsupportedEvent(event_name.to_owned()));
        }
        if data == "[DONE]" {
            if self.done || !self.stopped {
                return Err(DecodeError::DuplicateFinality);
            }
            self.done = true;
            return Ok(if self.usage_reported {
                Vec::new()
            } else {
                self.usage_reported = true;
                vec![ModelEvent::Usage(TokenUsage::Unavailable)]
            });
        }
        if self.done {
            return Err(DecodeError::DuplicateFinality);
        }

        let chunk: ChatChunk = serde_json::from_str(data)?;
        if chunk.choices.len() > 1 {
            return Err(DecodeError::UnsupportedEvent(
                "multiple_chat_choices".to_owned(),
            ));
        }
        let usage = chunk.usage.map(ChatUsage::into_semantic).transpose()?;
        let Some(choice) = chunk.choices.into_iter().next() else {
            let Some(usage) = usage else {
                return Ok(Vec::new());
            };
            if !self.stopped || std::mem::replace(&mut self.usage_reported, true) {
                return Err(DecodeError::DuplicateUsage);
            }
            return Ok(vec![ModelEvent::Usage(usage)]);
        };
        if self.stopped {
            return Err(DecodeError::DuplicateFinality);
        }
        if choice.index != 0 {
            return Err(DecodeError::UnsupportedEvent(format!(
                "chat_choice_{}",
                choice.index
            )));
        }

        let mut events = Vec::new();
        crate::wire::check_additive_fields(&choice.delta.extra, "chat_delta")?;
        if choice
            .delta
            .role
            .as_deref()
            .is_some_and(|role| role != "assistant")
        {
            return Err(DecodeError::UnsupportedEvent("chat_delta_role".to_owned()));
        }
        if choice
            .delta
            .reasoning_details
            .as_ref()
            .is_some_and(|value| value != &Value::Null && value != &serde_json::json!([]))
        {
            return Err(DecodeError::UnsupportedEvent(
                "chat_reasoning_details".to_owned(),
            ));
        }
        for (field, text) in [
            (
                ReasoningField::ReasoningContent,
                choice.delta.reasoning_content,
            ),
            (ReasoningField::Reasoning, choice.delta.reasoning),
            (ReasoningField::ReasoningText, choice.delta.reasoning_text),
        ] {
            let Some(reasoning) = text.filter(|text| !text.is_empty()) else {
                continue;
            };
            if self.reasoning_field.is_some_and(|prior| prior != field) {
                return Err(DecodeError::UnsupportedEvent(
                    "conflicting_chat_reasoning_fields".to_owned(),
                ));
            }
            self.retain(reasoning.len())?;
            events.push(ModelEvent::ReasoningDelta {
                position: output_position(0, 0)?,
                delta: reasoning,
            });
            if self.reasoning_field.replace(field).is_none()
                && field != ReasoningField::ReasoningContent
            {
                let payload = serde_json::to_string(&ChatReplay::Reasoning { field })?;
                if self.limits.max_replay_items == 0 {
                    return Err(DecodeError::TooManyReplayItems { limit: 0 });
                }
                if payload.len() > self.limits.max_replay_bytes {
                    return Err(DecodeError::RetainedReplayTooLarge {
                        limit: self.limits.max_replay_bytes,
                    });
                }
                let replay = ProviderReplay::new(self.compatibility.clone(), payload)
                    .map_err(DecodeError::Replay)?;
                events.push(ModelEvent::Replay {
                    position: output_position(0, 0)?,
                    replay,
                });
            }
        }
        if let Some(content) = choice.delta.content.filter(|text| !text.is_empty()) {
            self.retain(content.len())?;
            events.push(ModelEvent::TextDelta {
                position: output_position(0, 1)?,
                delta: content,
            });
        }
        if let Some(refusal) = choice.delta.refusal.filter(|text| !text.is_empty()) {
            self.saw_refusal = true;
            self.retain(refusal.len())?;
            events.push(ModelEvent::TextDelta {
                position: output_position(0, 1)?,
                delta: refusal,
            });
        }
        for fragment in choice.delta.tool_calls {
            self.tool_fragment(fragment)?;
        }
        if let Some(usage) = usage {
            if std::mem::replace(&mut self.usage_reported, true) {
                return Err(DecodeError::DuplicateUsage);
            }
            events.push(ModelEvent::Usage(usage));
        }
        if let Some(reason) = choice.finish_reason {
            events.extend(self.stop(reason)?);
        }
        Ok(events)
    }

    pub(crate) fn finish(self) -> Result<(), DecodeError> {
        if !self.stopped || !self.done || !self.calls.is_empty() {
            return Err(DecodeError::IncompleteStream);
        }
        Ok(())
    }

    fn retain(&mut self, added: usize) -> Result<(), DecodeError> {
        retain_bytes(
            &mut self.retained,
            added,
            self.limits.max_retained_output_bytes,
        )
    }

    fn tool_fragment(&mut self, fragment: ChatToolFragment) -> Result<(), DecodeError> {
        if !self.calls.contains_key(&fragment.index)
            && self.calls.len() >= self.limits.max_tool_calls
        {
            return Err(DecodeError::TooManyToolCalls {
                limit: self.limits.max_tool_calls,
            });
        }
        let call = self.calls.entry(fragment.index).or_default();
        merge_once(
            &mut call.id,
            fragment.id,
            fragment.index,
            "id",
            self.limits.max_tool_identity_bytes,
        )?;
        if let Some(function) = fragment.function {
            merge_once(
                &mut call.name,
                function.name,
                fragment.index,
                "name",
                self.limits.max_tool_identity_bytes,
            )?;
            if let Some(arguments) = function.arguments {
                let Some(next) = call.arguments.len().checked_add(arguments.len()) else {
                    return Err(DecodeError::ToolArgumentsTooLarge {
                        index: fragment.index,
                        limit: self.limits.max_tool_argument_bytes,
                    });
                };
                if next > self.limits.max_tool_argument_bytes {
                    return Err(DecodeError::ToolArgumentsTooLarge {
                        index: fragment.index,
                        limit: self.limits.max_tool_argument_bytes,
                    });
                }
                call.arguments.push_str(&arguments);
            }
        }
        Ok(())
    }

    fn stop(&mut self, reason: String) -> Result<Vec<ModelEvent>, DecodeError> {
        let stop = match reason.as_str() {
            "tool_calls" => {
                if self.calls.is_empty() {
                    return Err(DecodeError::IncompleteToolCall {
                        index: 0,
                        field: "call",
                    });
                }
                StopReason::ToolCalls
            }
            "stop" if self.saw_refusal => StopReason::Refused,
            "stop" => StopReason::EndOfTurn,
            "length" => StopReason::OutputLimit,
            "content_filter" => StopReason::Refused,
            _ => return Err(DecodeError::UnknownStopReason(reason)),
        };
        if stop != StopReason::ToolCalls && !self.calls.is_empty() {
            let index = *self.calls.keys().next().unwrap_or(&0);
            return Err(DecodeError::IncompleteToolCall {
                index,
                field: "finality",
            });
        }

        let mut events = Vec::new();
        if stop == StopReason::ToolCalls {
            let mut completed = Vec::with_capacity(self.calls.len());
            for (index, call) in std::mem::take(&mut self.calls) {
                let call = finish_call(index, call)?;
                if !self.completed_call_ids.insert(call.call_id.clone()) {
                    return Err(DecodeError::DuplicateToolCallId {
                        call_id: call.call_id,
                    });
                }
                completed.push(ModelEvent::Called {
                    position: output_position(0, index.saturating_add(2))?,
                    call,
                });
            }
            events.extend(completed);
        }
        self.stopped = true;
        events.push(ModelEvent::Stopped(stop));
        Ok(events)
    }
}

fn merge_once(
    current: &mut Option<String>,
    fragment: Option<String>,
    index: usize,
    field: &'static str,
    limit: usize,
) -> Result<(), DecodeError> {
    let Some(fragment) = fragment else {
        return Ok(());
    };
    if fragment.len() > limit {
        return Err(DecodeError::ToolIdentityTooLarge {
            index,
            field,
            limit,
        });
    }
    match current {
        Some(value) if value != &fragment => {
            Err(DecodeError::ConflictingToolFragment { index, field })
        }
        Some(_) => Ok(()),
        None => {
            *current = Some(fragment);
            Ok(())
        }
    }
}

fn finish_call(index: usize, call: CallAssembly) -> Result<ToolCall, DecodeError> {
    let id = required(call.id, index, "id")?;
    let name = required(call.name, index, "name")?;
    let call_id =
        ToolCallId::new(id).map_err(|_| DecodeError::IncompleteToolCall { index, field: "id" })?;
    Ok(ToolCall {
        call_id,
        name,
        arguments: call.arguments,
    })
}

fn required(
    value: Option<String>,
    index: usize,
    field: &'static str,
) -> Result<String, DecodeError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or(DecodeError::IncompleteToolCall { index, field })
}

#[derive(Deserialize)]
struct ChatChunk {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    index: usize,
    #[serde(default)]
    delta: ChatDelta,
    finish_reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct ChatDelta {
    role: Option<String>,
    reasoning: Option<String>,
    reasoning_text: Option<String>,
    reasoning_details: Option<Value>,
    content: Option<String>,
    reasoning_content: Option<String>,
    refusal: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ChatToolFragment>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Deserialize)]
struct ChatToolFragment {
    index: usize,
    id: Option<String>,
    function: Option<ChatFunctionFragment>,
}

#[derive(Deserialize)]
struct ChatFunctionFragment {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReasoningField {
    #[default]
    ReasoningContent,
    Reasoning,
    ReasoningText,
}
impl ReasoningField {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ReasoningContent => "reasoning_content",
            Self::Reasoning => "reasoning",
            Self::ReasoningText => "reasoning_text",
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ChatReplay {
    Reasoning { field: ReasoningField },
}
