//! Incremental Responses event state machine.

use std::collections::{BTreeMap, BTreeSet};

use plexmaton_agent::{ModelEvent, ProviderCodecId, ProviderReplay, StopReason};
use plexmaton_core::ToolCallId;
use serde_json::Value;

use super::call::CallAssembly;
use super::text::TextAssembly;
use super::usage::event_usage;
use super::wire::{object_field, optional_string, provider_failed, string_field, usize_field};
use crate::codec::{DecodeError, DecodeLimits, RESPONSES_CODEC_ID, retain_bytes};

#[derive(Debug)]
pub(crate) struct ResponsesDecoder {
    limits: DecodeLimits,
    retained: usize,
    replay_bytes: usize,
    replay_items: usize,
    calls: BTreeMap<usize, CallAssembly>,
    text_parts: BTreeMap<(usize, usize), TextAssembly>,
    finished_items: BTreeSet<usize>,
    finished_calls: BTreeSet<usize>,
    completed_call_ids: BTreeSet<ToolCallId>,
    saw_refusal: bool,
    stopped: bool,
}

impl ResponsesDecoder {
    pub(crate) fn new(limits: DecodeLimits) -> Self {
        Self {
            limits,
            retained: 0,
            replay_bytes: 0,
            replay_items: 0,
            calls: BTreeMap::new(),
            text_parts: BTreeMap::new(),
            finished_items: BTreeSet::new(),
            finished_calls: BTreeSet::new(),
            completed_call_ids: BTreeSet::new(),
            saw_refusal: false,
            stopped: false,
        }
    }

    pub(crate) fn push(
        &mut self,
        event_name: &str,
        data: &str,
    ) -> Result<Vec<ModelEvent>, DecodeError> {
        if self.stopped {
            return Err(DecodeError::DuplicateFinality);
        }
        let event: Value = serde_json::from_str(data)?;
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| DecodeError::UnsupportedEvent("missing_type".to_owned()))?;
        if event_name != "message" && event_name != event_type {
            return Err(DecodeError::ConflictingEventType {
                event: event_name.to_owned(),
                json_type: event_type.to_owned(),
            });
        }

        match event_type {
            "response.created"
            | "response.queued"
            | "response.in_progress"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done" => Ok(Vec::new()),
            "response.output_text.delta" => self.text_delta(&event, false),
            "response.output_text.done" => self.text_done(&event, "text", false),
            "response.refusal.delta" => self.text_delta(&event, true),
            "response.refusal.done" => self.text_done(&event, "refusal", true),
            "response.output_item.added" => {
                self.output_item_added(&event)?;
                Ok(Vec::new())
            }
            "response.function_call_arguments.delta" => {
                self.arguments_delta(&event)?;
                Ok(Vec::new())
            }
            "response.function_call_arguments.done" => {
                self.arguments_done(&event)?;
                Ok(Vec::new())
            }
            "response.output_item.done" => self.output_item_done(&event),
            "response.completed" => self.completed(&event),
            "response.incomplete" => self.incomplete(&event),
            "response.failed" | "error" => Err(provider_failed(&event)),
            _ => Err(DecodeError::UnsupportedEvent(event_type.to_owned())),
        }
    }

    pub(crate) fn finish(self) -> Result<(), DecodeError> {
        if !self.stopped || !self.calls.is_empty() {
            return Err(DecodeError::IncompleteStream);
        }
        Ok(())
    }

    fn text_delta(&mut self, event: &Value, refusal: bool) -> Result<Vec<ModelEvent>, DecodeError> {
        let delta = string_field(event, "delta")?.to_owned();
        if delta.is_empty() {
            return Ok(Vec::new());
        }
        let output_index = usize_field(event, "output_index")?;
        let content_index = usize_field(event, "content_index")?;
        retain_bytes(
            &mut self.retained,
            delta.len(),
            self.limits.max_retained_output_bytes,
        )?;
        self.text_part(output_index, content_index)?
            .append(&delta, output_index, content_index)?;
        if refusal {
            self.saw_refusal = true;
        }
        Ok(vec![ModelEvent::TextDelta(delta)])
    }

    fn text_done(
        &mut self,
        event: &Value,
        field: &'static str,
        refusal: bool,
    ) -> Result<Vec<ModelEvent>, DecodeError> {
        let output_index = usize_field(event, "output_index")?;
        let content_index = usize_field(event, "content_index")?;
        let complete = string_field(event, field)?;
        let unstreamed = self.text_part(output_index, content_index)?.finish(
            complete,
            output_index,
            content_index,
        )?;
        if refusal {
            self.saw_refusal = true;
        }
        if let Some(text) = unstreamed {
            retain_bytes(
                &mut self.retained,
                text.len(),
                self.limits.max_retained_output_bytes,
            )?;
            return Ok(vec![ModelEvent::TextDelta(text)]);
        }
        Ok(Vec::new())
    }

    fn output_item_added(&mut self, event: &Value) -> Result<(), DecodeError> {
        let index = usize_field(event, "output_index")?;
        let item = object_field(event, "item")?;
        match string_field(item, "type")? {
            "function_call" => {
                let limits = self.limits;
                let call = self.call(index)?;
                call.merge_identity(
                    optional_string(item, "id"),
                    optional_string(item, "call_id"),
                    optional_string(item, "name"),
                    index,
                )?;
                call.seed_arguments(optional_string(item, "arguments"), index, limits)
            }
            "reasoning" | "message" => Ok(()),
            other => Err(DecodeError::UnsupportedEvent(format!(
                "response.output_item.added:{other}"
            ))),
        }
    }

    fn arguments_delta(&mut self, event: &Value) -> Result<(), DecodeError> {
        let index = usize_field(event, "output_index")?;
        let item_id = optional_string(event, "item_id");
        let delta = string_field(event, "delta")?;
        let limits = self.limits;
        let call = self.call(index)?;
        call.merge_item_id(item_id, index)?;
        call.append_delta(delta, index, limits)
    }

    fn arguments_done(&mut self, event: &Value) -> Result<(), DecodeError> {
        let index = usize_field(event, "output_index")?;
        let item_id = optional_string(event, "item_id");
        let arguments = string_field(event, "arguments")?;
        let limits = self.limits;
        let call = self.call(index)?;
        call.merge_item_id(item_id, index)?;
        call.mark_arguments_done(arguments, index, limits)
    }

    fn output_item_done(&mut self, event: &Value) -> Result<Vec<ModelEvent>, DecodeError> {
        let index = usize_field(event, "output_index")?;
        if self.finished_items.contains(&index) {
            return Err(DecodeError::ConflictingToolFragment {
                index,
                field: "output_item_done",
            });
        }
        if self.finished_items.len() >= self.limits.max_output_items {
            return Err(DecodeError::TooManyOutputItems {
                limit: self.limits.max_output_items,
            });
        }
        let item = object_field(event, "item")?;
        let events = match string_field(item, "type")? {
            "reasoning" => self.reasoning_replay(item),
            "function_call" => self.function_call_done(index, item),
            "message" => Ok(Vec::new()),
            other => Err(DecodeError::UnsupportedEvent(format!(
                "response.output_item.done:{other}"
            ))),
        }?;
        self.finished_items.insert(index);
        Ok(events)
    }

    fn function_call_done(
        &mut self,
        index: usize,
        item: &Value,
    ) -> Result<Vec<ModelEvent>, DecodeError> {
        let limits = self.limits;
        let call = self.call(index)?;
        call.merge_identity(
            optional_string(item, "id"),
            optional_string(item, "call_id"),
            optional_string(item, "name"),
            index,
        )?;
        call.complete_arguments(string_field(item, "arguments")?, index, limits)?;
        let call = self
            .calls
            .remove(&index)
            .ok_or(DecodeError::IncompleteToolCall {
                index,
                field: "call",
            })?;
        let call = call.finish(index)?;
        if !self.completed_call_ids.insert(call.call_id.clone()) {
            return Err(DecodeError::DuplicateToolCallId {
                call_id: call.call_id,
            });
        }
        self.finished_calls.insert(index);
        Ok(vec![ModelEvent::Called(call)])
    }

    fn reasoning_replay(&mut self, item: &Value) -> Result<Vec<ModelEvent>, DecodeError> {
        if item
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_none()
        {
            return Err(DecodeError::UnsupportedEvent(
                "reasoning_without_encrypted_content".to_owned(),
            ));
        }
        if self.replay_items >= self.limits.max_replay_items {
            return Err(DecodeError::TooManyReplayItems {
                limit: self.limits.max_replay_items,
            });
        }
        let payload = serde_json::to_string(item)?;
        let Some(next) = self.replay_bytes.checked_add(payload.len()) else {
            return Err(DecodeError::RetainedReplayTooLarge {
                limit: self.limits.max_replay_bytes,
            });
        };
        if next > self.limits.max_replay_bytes {
            return Err(DecodeError::RetainedReplayTooLarge {
                limit: self.limits.max_replay_bytes,
            });
        }
        let codec = ProviderCodecId::new(RESPONSES_CODEC_ID).map_err(DecodeError::Replay)?;
        let replay = ProviderReplay::new(codec, payload).map_err(DecodeError::Replay)?;
        self.replay_bytes = next;
        self.replay_items += 1;
        Ok(vec![ModelEvent::Replay(replay)])
    }

    fn completed(&mut self, event: &Value) -> Result<Vec<ModelEvent>, DecodeError> {
        self.require_no_partial_call()?;
        self.require_no_partial_text()?;
        if let Some(status) = event
            .get("response")
            .and_then(|response| response.get("status"))
            .and_then(Value::as_str)
            && status != "completed"
        {
            return Err(DecodeError::UnknownStopReason(status.to_owned()));
        }
        self.stopped = true;
        let reason = if self.saw_refusal {
            StopReason::Refused
        } else if self.finished_calls.is_empty() {
            StopReason::EndOfTurn
        } else {
            StopReason::ToolCalls
        };
        Ok(vec![event_usage(event)?, ModelEvent::Stopped(reason)])
    }

    fn incomplete(&mut self, event: &Value) -> Result<Vec<ModelEvent>, DecodeError> {
        self.require_no_partial_call()?;
        self.require_no_partial_text()?;
        let reason = event
            .get("response")
            .and_then(|response| response.get("incomplete_details"))
            .and_then(|details| details.get("reason"))
            .and_then(Value::as_str)
            .or_else(|| {
                event
                    .get("incomplete_details")
                    .and_then(|details| details.get("reason"))
                    .and_then(Value::as_str)
            })
            .ok_or_else(|| DecodeError::UnknownStopReason("missing".to_owned()))?;
        let stop = match reason {
            "max_output_tokens" => StopReason::OutputLimit,
            "content_filter" => StopReason::Refused,
            other => return Err(DecodeError::UnknownStopReason(other.to_owned())),
        };
        self.stopped = true;
        Ok(vec![event_usage(event)?, ModelEvent::Stopped(stop)])
    }

    fn require_no_partial_call(&self) -> Result<(), DecodeError> {
        if let Some(index) = self.calls.keys().next() {
            return Err(DecodeError::IncompleteToolCall {
                index: *index,
                field: "output_item_done",
            });
        }
        Ok(())
    }

    fn require_no_partial_text(&self) -> Result<(), DecodeError> {
        if let Some(((output_index, content_index), _)) =
            self.text_parts.iter().find(|(_, text)| !text.is_done())
        {
            return Err(DecodeError::IncompleteOutputText {
                output_index: *output_index,
                content_index: *content_index,
            });
        }
        Ok(())
    }

    fn text_part(
        &mut self,
        output_index: usize,
        content_index: usize,
    ) -> Result<&mut TextAssembly, DecodeError> {
        let key = (output_index, content_index);
        if !self.text_parts.contains_key(&key)
            && self.text_parts.len() >= self.limits.max_output_items
        {
            return Err(DecodeError::TooManyOutputItems {
                limit: self.limits.max_output_items,
            });
        }
        Ok(self.text_parts.entry(key).or_default())
    }

    fn call(&mut self, index: usize) -> Result<&mut CallAssembly, DecodeError> {
        if self.finished_items.contains(&index) {
            return Err(DecodeError::ConflictingToolFragment {
                index,
                field: "fragment_after_output_item_done",
            });
        }
        if !self.calls.contains_key(&index) {
            let tracked = self.finished_calls.len().saturating_add(self.calls.len());
            if tracked >= self.limits.max_tool_calls {
                return Err(DecodeError::TooManyToolCalls {
                    limit: self.limits.max_tool_calls,
                });
            }
        }
        Ok(self.calls.entry(index).or_default())
    }
}
