//! Chat Completions request and stream grammar.

use std::collections::{BTreeMap, BTreeSet};

use plexmaton_agent::{ModelEvent, ModelRequest, RequestItem, StopReason, ToolCall};
use plexmaton_core::ToolCallId;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    FunctionTool, ProviderProfile,
    codec::{DecodeError, DecodeLimits, EncodeError, retain_bytes, tool_output},
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

#[derive(Debug)]
pub(crate) struct ChatDecoder {
    limits: DecodeLimits,
    retained: usize,
    calls: BTreeMap<usize, CallAssembly>,
    saw_refusal: bool,
    stopped: bool,
    done: bool,
}

#[derive(Debug, Default)]
struct CallAssembly {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ChatDecoder {
    pub(crate) fn new(limits: DecodeLimits) -> Self {
        Self {
            limits,
            retained: 0,
            calls: BTreeMap::new(),
            saw_refusal: false,
            stopped: false,
            done: false,
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
            return Ok(Vec::new());
        }
        if self.stopped || self.done {
            return Err(DecodeError::DuplicateFinality);
        }

        let chunk: ChatChunk = serde_json::from_str(data)?;
        if chunk.choices.len() > 1 {
            return Err(DecodeError::UnsupportedEvent(
                "multiple_chat_choices".to_owned(),
            ));
        }
        let Some(choice) = chunk.choices.into_iter().next() else {
            return Ok(Vec::new());
        };
        if choice.index != 0 {
            return Err(DecodeError::UnsupportedEvent(format!(
                "chat_choice_{}",
                choice.index
            )));
        }

        let mut events = Vec::new();
        if let Some(reasoning) = choice
            .delta
            .reasoning_content
            .filter(|text| !text.is_empty())
        {
            self.retain(reasoning.len())?;
            events.push(ModelEvent::ReasoningDelta(reasoning));
        }
        if let Some(content) = choice.delta.content.filter(|text| !text.is_empty()) {
            self.retain(content.len())?;
            events.push(ModelEvent::TextDelta(content));
        }
        if let Some(refusal) = choice.delta.refusal.filter(|text| !text.is_empty()) {
            self.saw_refusal = true;
            self.retain(refusal.len())?;
            events.push(ModelEvent::TextDelta(refusal));
        }
        for fragment in choice.delta.tool_calls {
            self.tool_fragment(fragment)?;
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
        merge_once(&mut call.id, fragment.id, fragment.index, "id")?;
        if let Some(function) = fragment.function {
            merge_once(&mut call.name, function.name, fragment.index, "name")?;
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
            for (index, call) in std::mem::take(&mut self.calls) {
                events.push(ModelEvent::Called(finish_call(index, call)?));
            }
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
) -> Result<(), DecodeError> {
    let Some(fragment) = fragment else {
        return Ok(());
    };
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
    content: Option<String>,
    reasoning_content: Option<String>,
    refusal: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ChatToolFragment>,
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
