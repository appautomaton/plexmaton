//! Explicit Messages lifecycle; unfinished blocks never become calls or replay capsules.

use std::collections::BTreeSet;

use plexmaton_agent::{ModelEvent, ReplayCompatibility, StopReason};
use plexmaton_core::ToolCallId;
use serde_json::Value;

use super::{block::Block, usage::MessagesUsage};
use crate::{
    DecodeError, DecodeLimits,
    codec::retain_bytes,
    wire::{object_field, provider_failed, string_field, usize_field},
};

#[derive(Clone, Copy, Debug)]
enum Phase {
    AwaitingStart,
    Streaming,
    Finishing(StopReason),
    Finished,
}

pub(crate) struct MessagesDecoder {
    limits: DecodeLimits,
    compatibility: ReplayCompatibility,
    phase: Phase,
    active: Option<Block>,
    next_index: usize,
    calls: BTreeSet<ToolCallId>,
    usage: MessagesUsage,
    retained: usize,
    replay_bytes: usize,
    replay_items: usize,
}

impl std::fmt::Debug for MessagesDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessagesDecoder")
            .field("phase", &self.phase)
            .field("next_index", &self.next_index)
            .finish_non_exhaustive()
    }
}

impl MessagesDecoder {
    pub(crate) fn new(limits: DecodeLimits, compatibility: ReplayCompatibility) -> Self {
        Self {
            limits,
            compatibility,
            phase: Phase::AwaitingStart,
            active: None,
            next_index: 0,
            calls: BTreeSet::new(),
            usage: MessagesUsage::default(),
            retained: 0,
            replay_bytes: 0,
            replay_items: 0,
        }
    }

    pub(crate) fn push(&mut self, name: &str, data: &str) -> Result<Vec<ModelEvent>, DecodeError> {
        let event: Value = serde_json::from_str(data)?;
        let kind = string_field(&event, "type")?;
        if name != "message" && name != kind {
            return Err(DecodeError::ConflictingEventType {
                event: name.to_owned(),
                json_type: kind.to_owned(),
            });
        }
        if kind == "ping" {
            return Ok(Vec::new());
        }
        if matches!(self.phase, Phase::Finished) {
            return Err(DecodeError::DuplicateFinality);
        }
        if kind == "error" {
            return Err(provider_failed(&event));
        }
        let events = match kind {
            "message_start" if matches!(self.phase, Phase::AwaitingStart) => {
                let message = object_field(&event, "message")?;
                if string_field(message, "role")? != "assistant"
                    || message
                        .get("content")
                        .and_then(Value::as_array)
                        .is_none_or(|parts| !parts.is_empty())
                {
                    return Err(DecodeError::UnsupportedEvent(
                        "message_start_content".to_owned(),
                    ));
                }
                self.phase = Phase::Streaming;
                vec![ModelEvent::Usage(self.usage.update(message.get("usage"))?)]
            }
            "content_block_start" if matches!(self.phase, Phase::Streaming) => {
                let index = usize_field(&event, "index")?;
                if self.active.is_some() || index != self.next_index {
                    return Err(DecodeError::ConflictingToolFragment {
                        index,
                        field: "content_block_start",
                    });
                }
                if index >= self.limits.max_output_items {
                    return Err(DecodeError::TooManyOutputItems {
                        limit: self.limits.max_output_items,
                    });
                }
                let (block, events) =
                    Block::start(index, object_field(&event, "content_block")?, self.limits)?;
                self.active = Some(block);
                events
            }
            "content_block_delta" if matches!(self.phase, Phase::Streaming) => {
                let index = usize_field(&event, "index")?;
                let active = self
                    .active
                    .as_mut()
                    .filter(|block| block.index == index)
                    .ok_or(DecodeError::IncompleteToolCall {
                        index,
                        field: "content_block_start",
                    })?;
                active.delta(object_field(&event, "delta")?, self.limits)?
            }
            "content_block_stop" if matches!(self.phase, Phase::Streaming) => {
                let index = usize_field(&event, "index")?;
                let active = self
                    .active
                    .take()
                    .filter(|block| block.index == index)
                    .ok_or(DecodeError::IncompleteToolCall {
                        index,
                        field: "content_block_start",
                    })?;
                self.next_index += 1;
                active.finish(&self.compatibility)?.into_iter().collect()
            }
            "message_delta"
                if !matches!(self.phase, Phase::AwaitingStart) && self.active.is_none() =>
            {
                let delta = object_field(&event, "delta")?;
                if let Some(reason) = delta.get("stop_reason").filter(|value| !value.is_null()) {
                    let reason = reason
                        .as_str()
                        .ok_or_else(|| DecodeError::UnknownStopReason("invalid".to_owned()))?;
                    let stop = match reason {
                        "end_turn" | "stop_sequence" => StopReason::EndOfTurn,
                        "tool_use" => StopReason::ToolCalls,
                        "max_tokens" => StopReason::OutputLimit,
                        "refusal" => StopReason::Refused,
                        "model_context_window_exceeded" => StopReason::ContextLimit,
                        "pause_turn" => {
                            return Err(DecodeError::UnsupportedEvent(
                                "messages_pause_turn".to_owned(),
                            ));
                        }
                        _ => return Err(DecodeError::UnknownStopReason(reason.to_owned())),
                    };
                    if let Phase::Finishing(prior) = self.phase
                        && prior != stop
                    {
                        return Err(DecodeError::DuplicateFinality);
                    }
                    self.phase = Phase::Finishing(stop);
                }
                vec![ModelEvent::Usage(self.usage.update(event.get("usage"))?)]
            }
            "message_stop" if self.active.is_none() => {
                let Phase::Finishing(reason) = self.phase else {
                    return Err(DecodeError::IncompleteStream);
                };
                if (reason == StopReason::ToolCalls) != !self.calls.is_empty() {
                    return Err(DecodeError::IncompleteToolCall {
                        index: self.next_index,
                        field: "stop_reason",
                    });
                }
                self.phase = Phase::Finished;
                vec![ModelEvent::Stopped(reason)]
            }
            _ => return Err(DecodeError::UnsupportedEvent(kind.to_owned())),
        };
        self.validate_events(&events)?;
        Ok(events)
    }

    fn validate_events(&mut self, events: &[ModelEvent]) -> Result<(), DecodeError> {
        for event in events {
            match event {
                ModelEvent::TextDelta { delta, .. } | ModelEvent::ReasoningDelta { delta, .. } => {
                    retain_bytes(
                        &mut self.retained,
                        delta.len(),
                        self.limits.max_retained_output_bytes,
                    )?
                }
                ModelEvent::Replay { replay, .. } => {
                    if self.replay_items >= self.limits.max_replay_items {
                        return Err(DecodeError::TooManyReplayItems {
                            limit: self.limits.max_replay_items,
                        });
                    }
                    self.replay_bytes = self
                        .replay_bytes
                        .checked_add(replay.payload().len())
                        .filter(|bytes| *bytes <= self.limits.max_replay_bytes)
                        .ok_or(DecodeError::RetainedReplayTooLarge {
                            limit: self.limits.max_replay_bytes,
                        })?;
                    self.replay_items += 1;
                }
                ModelEvent::Called { call, .. } => {
                    if self.calls.len() >= self.limits.max_tool_calls {
                        return Err(DecodeError::TooManyToolCalls {
                            limit: self.limits.max_tool_calls,
                        });
                    }
                    if !self.calls.insert(call.call_id.clone()) {
                        return Err(DecodeError::DuplicateToolCallId {
                            call_id: call.call_id.clone(),
                        });
                    }
                }
                ModelEvent::Usage(_) | ModelEvent::Stopped(_) => {}
            }
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<(), DecodeError> {
        if !matches!(self.phase, Phase::Finished) || self.active.is_some() {
            return Err(DecodeError::IncompleteStream);
        }
        Ok(())
    }
}
