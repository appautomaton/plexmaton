//! One model step's ordered, bounded output assembly.

use std::collections::{BTreeMap, btree_map::Entry};

use plexmaton_core::{SessionEvent, TranscriptItemId, TranscriptRole, TurnId};

use crate::interface::Reaction;
use crate::journal::JournalEntryPayload;
use crate::model::{
    AssistantBlock, AssistantOutput, AssistantReplay, MAX_ASSISTANT_TEXT_BYTES,
    MAX_ASSISTANT_TOOL_ARGUMENT_BYTES, MAX_TOOL_IDENTITY_BYTES, ModelOutputPosition, ModelStepId,
    ProviderReplay,
};
use crate::record::Record;
use crate::tools::ToolCall;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Step {
    turn_id: TurnId,
    outputs: BTreeMap<ModelOutputPosition, PendingOutput>,
    replay: BTreeMap<ModelOutputPosition, ProviderReplay>,
    warnings: Vec<String>,
    index: u16,
    usage_reported: bool,
    semantic_text_bytes: usize,
    tool_argument_bytes: usize,
    replay_bytes: usize,
    last_visible_output_position: Option<ModelOutputPosition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingOutput {
    Text(StreamedText),
    Reasoning(StreamedText),
    ToolCall {
        item_id: TranscriptItemId,
        call: ToolCall,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StreamedText {
    role: TranscriptRole,
    item: TranscriptItemId,
    opened: bool,
    revision: u64,
    text: String,
}

impl StreamedText {
    const fn new(role: TranscriptRole, item: TranscriptItemId) -> Self {
        Self {
            role,
            item,
            opened: false,
            revision: 0,
            text: String::new(),
        }
    }

    fn append(&mut self, record: &mut Record, reaction: &mut Reaction, delta: String) {
        if !std::mem::replace(&mut self.opened, true) {
            record.emit(
                reaction,
                SessionEvent::TranscriptItemStarted {
                    agent_id: record.agent_id().clone(),
                    item_id: self.item.clone(),
                    role: self.role,
                },
            );
        }
        self.text.push_str(&delta);
        self.revision = self.revision.saturating_add(1);
        record.emit(
            reaction,
            SessionEvent::TranscriptDelta {
                agent_id: record.agent_id().clone(),
                item_id: self.item.clone(),
                item_revision: self.revision,
                text: delta,
            },
        );
    }

    fn finalize(&self, record: &mut Record, reaction: &mut Reaction) {
        if self.opened {
            record.emit(
                reaction,
                SessionEvent::TranscriptItemFinalized {
                    agent_id: record.agent_id().clone(),
                    item_id: self.item.clone(),
                    item_revision: self.revision.saturating_add(1),
                },
            );
        }
    }
}

impl Step {
    pub(crate) fn new(turn_id: TurnId, index: u16) -> Self {
        Self {
            turn_id,
            outputs: BTreeMap::new(),
            replay: BTreeMap::new(),
            warnings: Vec::new(),
            index,
            usage_reported: false,
            semantic_text_bytes: 0,
            tool_argument_bytes: 0,
            replay_bytes: 0,
            last_visible_output_position: None,
        }
    }

    pub(crate) fn index(&self) -> u16 {
        self.index
    }

    pub(crate) fn append(
        &mut self,
        record: &mut Record,
        reaction: &mut Reaction,
        position: ModelOutputPosition,
        delta: String,
    ) -> Result<(), StepAssemblyError> {
        self.append_text(record, reaction, position, TranscriptRole::Assistant, delta)
    }

    pub(crate) fn append_reasoning(
        &mut self,
        record: &mut Record,
        reaction: &mut Reaction,
        position: ModelOutputPosition,
        delta: String,
    ) -> Result<(), StepAssemblyError> {
        self.append_text(record, reaction, position, TranscriptRole::Reasoning, delta)
    }

    fn append_text(
        &mut self,
        record: &mut Record,
        reaction: &mut Reaction,
        position: ModelOutputPosition,
        role: TranscriptRole,
        delta: String,
    ) -> Result<(), StepAssemblyError> {
        let semantic_text_bytes = self
            .semantic_text_bytes
            .checked_add(delta.len())
            .ok_or(StepAssemblyError::TextTooLarge)?;
        if semantic_text_bytes > MAX_ASSISTANT_TEXT_BYTES {
            return Err(StepAssemblyError::TextTooLarge);
        }
        self.reserve_visible_output_position(position)?;
        let item = Record::stream_item_id(&self.turn_id, self.index, role, position);
        let output = self.outputs.entry(position).or_insert_with(|| match role {
            TranscriptRole::Assistant => PendingOutput::Text(StreamedText::new(role, item)),
            TranscriptRole::Reasoning => PendingOutput::Reasoning(StreamedText::new(role, item)),
            TranscriptRole::User | TranscriptRole::System => unreachable!("provider text role"),
        });
        let text = match (role, output) {
            (TranscriptRole::Assistant, PendingOutput::Text(text))
            | (TranscriptRole::Reasoning, PendingOutput::Reasoning(text)) => text,
            _ => return Err(StepAssemblyError::ConflictingPosition),
        };
        text.append(record, reaction, delta);
        self.semantic_text_bytes = semantic_text_bytes;
        Ok(())
    }

    pub(crate) fn retain_replay(
        &mut self,
        position: ModelOutputPosition,
        replay: ProviderReplay,
    ) -> Result<(), StepAssemblyError> {
        self.reserve_output_position(position)?;
        if self
            .replay
            .values()
            .next()
            .is_some_and(|existing| existing.compatible_with() != replay.compatible_with())
        {
            return Err(StepAssemblyError::MixedReplayCompatibility);
        }
        if self.replay.contains_key(&position) {
            return Err(StepAssemblyError::DuplicateReplayPosition);
        }
        let replay_bytes = self
            .replay_bytes
            .checked_add(replay.payload().len())
            .ok_or(StepAssemblyError::ReplayTooLarge)?;
        if replay_bytes > crate::MAX_PROVIDER_REPLAY_BYTES {
            return Err(StepAssemblyError::ReplayTooLarge);
        }
        let item = Record::stream_item_id(
            &self.turn_id,
            self.index,
            TranscriptRole::Reasoning,
            position,
        );
        match self.outputs.entry(position).or_insert_with(|| {
            PendingOutput::Reasoning(StreamedText::new(TranscriptRole::Reasoning, item))
        }) {
            PendingOutput::Reasoning(_) => {}
            PendingOutput::Text(_) | PendingOutput::ToolCall { .. } => {
                return Err(StepAssemblyError::ConflictingPosition);
            }
        }
        self.replay.insert(position, replay);
        self.replay_bytes = replay_bytes;
        Ok(())
    }

    pub(crate) fn collect(
        &mut self,
        record: &Record,
        position: ModelOutputPosition,
        call: ToolCall,
    ) -> Result<(), StepAssemblyError> {
        if self.contains_call(&call.call_id) {
            return Err(StepAssemblyError::DuplicateToolCallId);
        }
        if call.call_id.as_str().len() > MAX_TOOL_IDENTITY_BYTES
            || call.name.len() > MAX_TOOL_IDENTITY_BYTES
        {
            return Err(StepAssemblyError::ToolIdentityTooLarge);
        }
        let tool_argument_bytes = self
            .tool_argument_bytes
            .checked_add(call.arguments.len())
            .ok_or(StepAssemblyError::ToolArgumentsTooLarge)?;
        if tool_argument_bytes > MAX_ASSISTANT_TOOL_ARGUMENT_BYTES {
            return Err(StepAssemblyError::ToolArgumentsTooLarge);
        }
        self.reserve_output_position(position)?;
        let item_id = record.tool_item_id(&call.call_id);
        match self.outputs.entry(position) {
            Entry::Vacant(entry) => {
                entry.insert(PendingOutput::ToolCall { item_id, call });
                self.tool_argument_bytes = tool_argument_bytes;
                Ok(())
            }
            Entry::Occupied(_) => Err(StepAssemblyError::ConflictingPosition),
        }
    }

    pub(crate) fn contains_call(&self, call_id: &plexmaton_core::ToolCallId) -> bool {
        self.outputs.values().any(|output| {
            matches!(output, PendingOutput::ToolCall { call, .. } if &call.call_id == call_id)
        })
    }

    fn reserve_output_position(
        &self,
        position: ModelOutputPosition,
    ) -> Result<(), StepAssemblyError> {
        if !self.outputs.contains_key(&position) && self.outputs.len() >= usize::from(u16::MAX) {
            return Err(StepAssemblyError::TooManyOutputBlocks);
        }
        Ok(())
    }

    fn reserve_visible_output_position(
        &mut self,
        position: ModelOutputPosition,
    ) -> Result<(), StepAssemblyError> {
        self.reserve_output_position(position)?;
        if !self.outputs.contains_key(&position) {
            if self
                .last_visible_output_position
                .is_some_and(|prior| position < prior)
            {
                return Err(StepAssemblyError::OutOfOrderOutputPosition);
            }
            self.last_visible_output_position = Some(position);
        }
        Ok(())
    }

    pub(crate) fn defer_warning(&mut self, message: &str) {
        self.warnings.push(message.to_owned());
    }

    pub(crate) fn mark_usage_reported(&mut self) -> bool {
        !std::mem::replace(&mut self.usage_reported, true)
    }

    pub(crate) fn close(
        self,
        record: &mut Record,
        reaction: &mut Reaction,
    ) -> (Vec<ToolCall>, Vec<String>) {
        self.close_retained(record, reaction)
    }

    pub(crate) fn abort(mut self, record: &mut Record, reaction: &mut Reaction) -> Vec<String> {
        self.outputs
            .retain(|_, output| !matches!(output, PendingOutput::ToolCall { .. }));
        let (calls, warnings) = self.close_retained(record, reaction);
        debug_assert!(
            calls.is_empty(),
            "aborted steps retain no undispatched calls"
        );
        warnings
    }

    fn close_retained(
        self,
        record: &mut Record,
        reaction: &mut Reaction,
    ) -> (Vec<ToolCall>, Vec<String>) {
        if self.outputs.is_empty() {
            return (Vec::new(), self.warnings);
        }
        let mut positions = BTreeMap::new();
        let mut blocks = Vec::with_capacity(self.outputs.len());
        let mut streamed = Vec::new();
        for (position, output) in self.outputs {
            let block = match output {
                PendingOutput::Text(text) => {
                    if !text.opened {
                        continue;
                    }
                    streamed.push(text.clone());
                    AssistantBlock::Text {
                        item_id: text.item,
                        text: text.text,
                    }
                }
                PendingOutput::Reasoning(text) => {
                    if !text.opened && !self.replay.contains_key(&position) {
                        continue;
                    }
                    streamed.push(text.clone());
                    AssistantBlock::Reasoning {
                        item_id: text.item,
                        text: text.text,
                    }
                }
                PendingOutput::ToolCall { item_id, call } => {
                    AssistantBlock::ToolCall { item_id, call }
                }
            };
            let block_index = u16::try_from(blocks.len())
                .unwrap_or_else(|_| unreachable!("provider output block count is bounded"));
            positions.insert(position, block_index);
            blocks.push(block);
        }
        if blocks.is_empty() {
            return (Vec::new(), self.warnings);
        }
        let replay =
            AssistantReplay::from_positioned(self.replay.into_iter().map(|(position, replay)| {
                let block = positions
                    .get(&position)
                    .copied()
                    .unwrap_or_else(|| unreachable!("replay creates its reasoning anchor"));
                (block, replay)
            }))
            .unwrap_or_else(|error| unreachable!("step validates replay assembly: {error}"));
        let output = AssistantOutput::new(blocks, replay)
            .unwrap_or_else(|error| unreachable!("step validates assistant output: {error}"));
        let calls = output.tool_calls().cloned().collect();
        record.commit(
            JournalEntryPayload::AssistantOutput {
                agent_id: record.agent_id().clone(),
                step_id: ModelStepId::new(self.turn_id, self.index),
                output,
            },
            reaction,
        );
        for text in streamed {
            text.finalize(record, reaction);
        }
        (calls, self.warnings)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StepAssemblyError {
    ConflictingPosition,
    DuplicateReplayPosition,
    MixedReplayCompatibility,
    DuplicateToolCallId,
    TextTooLarge,
    ToolIdentityTooLarge,
    ToolArgumentsTooLarge,
    ReplayTooLarge,
    TooManyOutputBlocks,
    OutOfOrderOutputPosition,
}

impl StepAssemblyError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::ConflictingPosition => "provider output position changed semantic kind",
            Self::DuplicateReplayPosition => "provider replay position was repeated",
            Self::MixedReplayCompatibility => {
                "one model output mixed incompatible provider replay realms"
            }
            Self::DuplicateToolCallId => "provider repeated a tool call identity",
            Self::TextTooLarge => "provider output exceeded the step's semantic text byte bound",
            Self::ToolIdentityTooLarge => "provider tool identity exceeded its byte bound",
            Self::ToolArgumentsTooLarge => {
                "provider tool calls exceeded the step's aggregate argument byte bound"
            }
            Self::ReplayTooLarge => "provider replay exceeded the step's aggregate byte bound",
            Self::TooManyOutputBlocks => "provider output exceeded the step's block ordinal space",
            Self::OutOfOrderOutputPosition => {
                "provider emitted a new output position before one already shown"
            }
        }
    }
}
