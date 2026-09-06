//! Bounded, non-streaming semantic collection for a compaction-owned provider attempt.

use std::{collections::BTreeMap, future};

use plexmaton_agent::{
    AssistantBlock, AssistantOutput, AssistantReplay, CompactionAttemptFinished, CompactionFailure,
    CompactionInputMode, CompactionOutcome, MAX_ASSISTANT_TEXT_BYTES,
    MAX_ASSISTANT_TOOL_ARGUMENT_BYTES, MAX_COMPACTION_SUMMARY_BYTES, MAX_PROVIDER_REPLAY_BYTES,
    MAX_REQUESTED_TOOL_ARGUMENT_BYTES, MAX_TOOL_IDENTITY_BYTES, ModelError, ModelEvent,
    ModelOutputPosition, ProviderReplay, RequestAttemptId, StopReason, ToolCall,
};
use plexmaton_core::TranscriptItemId;
use plexmaton_provider::CompactionInput;
use tokio_util::sync::CancellationToken;

use super::{ProviderHttp, timing::AttemptReport};
use crate::runtime::ModelCompletion;

// Summary output has no legitimate need for an unbounded set of fragments. This also bounds
// aggregate per-block identities and tool names in the durable failed-attempt audit (CPL-6).
const MAX_SUMMARY_BLOCKS: usize = 256;

impl ProviderHttp {
    pub(super) async fn perform_summary(
        &self,
        attempt_id: RequestAttemptId,
        input: CompactionInput,
        max_summary_bytes: usize,
        cancellation: CancellationToken,
    ) -> CompactionAttemptFinished {
        let request = input.into_request();
        let mut collector = SummaryCollector::new(attempt_id.clone(), max_summary_bytes);
        let report = self
            .perform_events(attempt_id, request, cancellation, |output| {
                collector.push(output.into_event());
                future::ready(())
            })
            .await;
        collector.finish(report)
    }
}

pub(super) struct SummaryCollector {
    attempt_id: RequestAttemptId,
    max_summary_bytes: usize,
    blocks: BTreeMap<ModelOutputPosition, AssistantBlock>,
    replay: BTreeMap<ModelOutputPosition, ProviderReplay>,
    text_bytes: usize,
    summary_bytes: usize,
    argument_bytes: usize,
    replay_bytes: usize,
    rejection: Option<CompactionFailure>,
}

impl SummaryCollector {
    pub(super) fn new(attempt_id: RequestAttemptId, max_summary_bytes: usize) -> Self {
        Self {
            attempt_id,
            max_summary_bytes: max_summary_bytes.min(MAX_COMPACTION_SUMMARY_BYTES),
            blocks: BTreeMap::new(),
            replay: BTreeMap::new(),
            text_bytes: 0,
            summary_bytes: 0,
            argument_bytes: 0,
            replay_bytes: 0,
            rejection: None,
        }
    }

    pub(super) fn push(&mut self, event: ModelEvent) {
        let result = match event {
            ModelEvent::TextDelta { position, delta } => self.text(position, delta, false),
            ModelEvent::ReasoningDelta { position, delta } => self.text(position, delta, true),
            ModelEvent::Replay { position, replay } => self.replay(position, replay),
            ModelEvent::Called { position, call } => {
                self.rejection
                    .get_or_insert(CompactionFailure::ToolCallOutput);
                self.tool_call(position, call)
            }
            ModelEvent::Usage(_) | ModelEvent::Stopped(_) => {
                unreachable!("the shared HTTP driver consumes accounting and terminal events")
            }
        };
        if let Err(reason) = result {
            self.rejection.get_or_insert(reason);
        }
    }

    fn reserve(&self, position: ModelOutputPosition) -> Result<(), CompactionFailure> {
        if !self.blocks.contains_key(&position) && self.blocks.len() >= MAX_SUMMARY_BLOCKS {
            Err(CompactionFailure::OutputTooLarge)
        } else {
            Ok(())
        }
    }

    fn item_id(&self, position: ModelOutputPosition) -> TranscriptItemId {
        TranscriptItemId::new(format!(
            "{}-summary-{}-{}",
            self.attempt_id,
            position.item(),
            position.part()
        ))
        .unwrap_or_else(|error| unreachable!("a scoped output identity is nonempty: {error}"))
    }

    fn text(
        &mut self,
        position: ModelOutputPosition,
        delta: String,
        reasoning: bool,
    ) -> Result<(), CompactionFailure> {
        if delta.is_empty() {
            return Ok(());
        }
        self.reserve(position)?;
        let total = self
            .text_bytes
            .checked_add(delta.len())
            .filter(|&bytes| bytes <= MAX_ASSISTANT_TEXT_BYTES)
            .ok_or(CompactionFailure::OutputTooLarge)?;
        let item_id = self.item_id(position);
        let block = self
            .blocks
            .entry(position)
            .or_insert_with(|| AssistantBlock::ReplayOnly { item_id });
        if let AssistantBlock::ReplayOnly { item_id } = block {
            *block = if reasoning {
                AssistantBlock::Reasoning {
                    item_id: item_id.clone(),
                    text: String::new(),
                }
            } else {
                AssistantBlock::Text {
                    item_id: item_id.clone(),
                    text: String::new(),
                }
            };
        }
        let text = match (reasoning, block) {
            (false, AssistantBlock::Text { text, .. })
            | (true, AssistantBlock::Reasoning { text, .. }) => text,
            _ => return Err(CompactionFailure::Malformed),
        };
        text.push_str(&delta);
        self.text_bytes = total;
        if !reasoning {
            self.summary_bytes += delta.len();
            if self.summary_bytes > self.max_summary_bytes {
                self.rejection
                    .get_or_insert(CompactionFailure::OutputTooLarge);
            }
        }
        Ok(())
    }

    fn replay(
        &mut self,
        position: ModelOutputPosition,
        replay: ProviderReplay,
    ) -> Result<(), CompactionFailure> {
        self.reserve(position)?;
        if self.replay.contains_key(&position)
            || self
                .replay
                .values()
                .next()
                .is_some_and(|old| old.compatible_with() != replay.compatible_with())
        {
            return Err(CompactionFailure::Malformed);
        }
        let bytes = self
            .replay_bytes
            .checked_add(replay.payload().len())
            .filter(|&bytes| bytes <= MAX_PROVIDER_REPLAY_BYTES)
            .ok_or(CompactionFailure::OutputTooLarge)?;
        let item_id = self.item_id(position);
        self.blocks
            .entry(position)
            .or_insert(AssistantBlock::ReplayOnly { item_id });
        self.replay.insert(position, replay);
        self.replay_bytes = bytes;
        Ok(())
    }

    fn tool_call(
        &mut self,
        position: ModelOutputPosition,
        call: ToolCall,
    ) -> Result<(), CompactionFailure> {
        self.reserve(position)?;
        if self.blocks.contains_key(&position)
            || self.blocks.values().any(|block| {
                block
                    .tool_call()
                    .is_some_and(|old| old.call_id == call.call_id)
            })
        {
            return Err(CompactionFailure::Malformed);
        }
        if call.call_id.as_str().len() > MAX_TOOL_IDENTITY_BYTES
            || call.name.len() > MAX_TOOL_IDENTITY_BYTES
            || call.arguments.len() > MAX_REQUESTED_TOOL_ARGUMENT_BYTES
        {
            return Err(CompactionFailure::OutputTooLarge);
        }
        let bytes = self
            .argument_bytes
            .checked_add(call.arguments.len())
            .filter(|&bytes| bytes <= MAX_ASSISTANT_TOOL_ARGUMENT_BYTES)
            .ok_or(CompactionFailure::OutputTooLarge)?;
        self.blocks.insert(
            position,
            AssistantBlock::ToolCall {
                item_id: self.item_id(position),
                call,
            },
        );
        self.argument_bytes = bytes;
        Ok(())
    }

    pub(super) fn finish(mut self, report: AttemptReport) -> CompactionAttemptFinished {
        let rejection = self.rejection.take();
        let output = self.into_output();
        let (output, assembly_error) = match output {
            Ok(output) => (output, None),
            Err(error) => (None, Some(error)),
        };
        let rejection = match report.completion {
            ModelCompletion::Cancelled => Some(CompactionFailure::Cancelled),
            ModelCompletion::Failed(error) => Some(model_failure(&error)),
            ModelCompletion::Stopped(StopReason::EndOfTurn) => rejection.or(assembly_error),
            ModelCompletion::Stopped(reason) => Some(stop_failure(reason)),
        };
        let outcome = match (rejection, output) {
            (None, Some(output)) if has_summary_text(&output) => {
                CompactionOutcome::Complete { output }
            }
            (reason, output) => CompactionOutcome::Failed {
                kind: reason.unwrap_or(CompactionFailure::EmptyOutput),
                output,
            },
        };
        CompactionAttemptFinished::new(report.terminal, CompactionInputMode::Verbatim, outcome)
            .unwrap_or_else(|error| {
                unreachable!("collector output and provider audit have been validated: {error:?}")
            })
    }

    fn into_output(self) -> Result<Option<AssistantOutput>, CompactionFailure> {
        if self.blocks.is_empty() {
            return Ok(None);
        }
        let mut blocks = Vec::with_capacity(self.blocks.len());
        let mut replay = Vec::new();
        let mut source_replay = self.replay;
        for (position, block) in self.blocks {
            if let Some(payload) = source_replay.remove(&position) {
                let index = u16::try_from(blocks.len())
                    .unwrap_or_else(|_| unreachable!("summary blocks have a fixed small bound"));
                replay.push((index, payload));
            }
            blocks.push(block);
        }
        let replay =
            AssistantReplay::from_positioned(replay).map_err(|_| CompactionFailure::Malformed)?;
        AssistantOutput::new(blocks, replay)
            .map(Some)
            .map_err(|_| CompactionFailure::Malformed)
    }
}

fn has_summary_text(output: &AssistantOutput) -> bool {
    output
        .blocks()
        .iter()
        .any(|block| matches!(block, AssistantBlock::Text { text, .. } if !text.trim().is_empty()))
}

fn model_failure(error: &ModelError) -> CompactionFailure {
    match error {
        ModelError::Transport { .. } => CompactionFailure::TransportFailed,
        ModelError::ContextTooLong => CompactionFailure::ContextTooLong,
        ModelError::Malformed { .. } => CompactionFailure::Malformed,
        ModelError::ProviderFailed { .. } | ModelError::RateLimited { .. } => {
            CompactionFailure::ProviderFailed
        }
    }
}

fn stop_failure(reason: StopReason) -> CompactionFailure {
    match reason {
        StopReason::ToolCalls => CompactionFailure::ToolCallOutput,
        StopReason::OutputLimit => CompactionFailure::OutputLimit,
        StopReason::ContextLimit => CompactionFailure::ContextTooLong,
        StopReason::Refused => CompactionFailure::Refused,
        StopReason::Unspecified => CompactionFailure::Malformed,
        StopReason::EndOfTurn => unreachable!("successful stop is handled separately"),
    }
}

#[cfg(test)]
mod tests;
