use std::collections::BTreeSet;

use plexmaton_core::{ConversationEntryId, ToolCallId, TranscriptItemId};
use serde::{Deserialize, Serialize};

use super::{ProviderReplay, ReplayCompatibility};
use crate::{SkillActivation, ToolCall, ToolOutcome};

#[cfg(test)]
mod tests;

mod error;
pub use error::ContextError;

/// Maximum aggregate raw tool-argument bytes retained in one assistant output.
///
/// Individual calls keep the admission boundary's 64 KiB limit. This second bound prevents a
/// parallel batch from multiplying that allowance until one canonical journal record no longer
/// fits its storage envelope.
pub const MAX_ASSISTANT_TOOL_ARGUMENT_BYTES: usize = 512 * 1024;

/// Maximum aggregate visible text and plaintext reasoning retained in one assistant output.
pub const MAX_ASSISTANT_TEXT_BYTES: usize = 1024 * 1024;

/// Maximum bytes retained for one provider-supplied tool call identity or name.
pub const MAX_TOOL_IDENTITY_BYTES: usize = 1024;

/// Provider output ordering before it is normalized into dense assistant blocks.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ModelOutputPosition {
    item: u16,
    part: u16,
}

impl ModelOutputPosition {
    #[must_use]
    pub const fn new(item: u16, part: u16) -> Self {
        Self { item, part }
    }

    #[must_use]
    pub const fn item(self) -> u16 {
        self.item
    }

    #[must_use]
    pub const fn part(self) -> u16 {
        self.part
    }
}

/// One semantic block in the exact order a model produced it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AssistantBlock {
    /// A provider part with replay data but no visible semantic content.
    ReplayOnly { item_id: TranscriptItemId },
    Text {
        item_id: TranscriptItemId,
        text: String,
    },
    Reasoning {
        item_id: TranscriptItemId,
        text: String,
    },
    ToolCall {
        item_id: TranscriptItemId,
        call: ToolCall,
    },
}

impl AssistantBlock {
    #[must_use]
    pub const fn item_id(&self) -> &TranscriptItemId {
        match self {
            Self::Text { item_id, .. }
            | Self::Reasoning { item_id, .. }
            | Self::ReplayOnly { item_id }
            | Self::ToolCall { item_id, .. } => item_id,
        }
    }

    #[must_use]
    pub const fn tool_call(&self) -> Option<&ToolCall> {
        match self {
            Self::ToolCall { call, .. } => Some(call),
            Self::Text { .. } | Self::Reasoning { .. } | Self::ReplayOnly { .. } => None,
        }
    }
}

/// Opaque provider payload attached to one dense assistant block ordinal.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct BlockReplay {
    block: u16,
    payload: String,
}

impl std::fmt::Debug for BlockReplay {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BlockReplay")
            .field("block", &self.block)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}

impl BlockReplay {
    #[must_use]
    pub const fn block(&self) -> u16 {
        self.block
    }

    #[must_use]
    pub fn payload(&self) -> &str {
        &self.payload
    }
}

/// Output-level replay sidecar whose compatibility applies to every attachment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssistantReplay {
    compatible_with: ReplayCompatibility,
    attachments: Box<[BlockReplay]>,
}

impl AssistantReplay {
    pub fn from_positioned(
        positioned: impl IntoIterator<Item = (u16, ProviderReplay)>,
    ) -> Result<Option<Self>, ContextError> {
        let mut positioned = positioned.into_iter();
        let Some((first_block, first)) = positioned.next() else {
            return Ok(None);
        };
        let (compatible_with, payload) = first.into_parts();
        let mut attachments = vec![BlockReplay {
            block: first_block,
            payload,
        }];
        for (block, replay) in positioned {
            let (found, payload) = replay.into_parts();
            if found != compatible_with {
                return Err(ContextError::MixedReplayCompatibility);
            }
            attachments.push(BlockReplay { block, payload });
        }
        Self::new(compatible_with, attachments).map(Some)
    }

    pub fn new(
        compatible_with: ReplayCompatibility,
        attachments: Vec<BlockReplay>,
    ) -> Result<Self, ContextError> {
        if attachments.is_empty() {
            return Err(ContextError::EmptyReplay);
        }
        let mut prior = None;
        let mut bytes = 0_usize;
        for attachment in &attachments {
            if attachment.payload.is_empty() {
                return Err(ContextError::EmptyReplayPayload);
            }
            if prior.is_some_and(|prior| prior >= attachment.block) {
                return Err(ContextError::UnorderedReplayAnchor);
            }
            prior = Some(attachment.block);
            bytes = bytes
                .checked_add(attachment.payload.len())
                .ok_or(ContextError::ReplayTooLarge)?;
        }
        if bytes > super::MAX_PROVIDER_REPLAY_BYTES {
            return Err(ContextError::ReplayTooLarge);
        }
        Ok(Self {
            compatible_with,
            attachments: attachments.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn compatible_with(&self) -> &ReplayCompatibility {
        &self.compatible_with
    }

    #[must_use]
    pub fn attachments(&self) -> &[BlockReplay] {
        &self.attachments
    }
}

impl<'de> Deserialize<'de> for AssistantReplay {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            compatible_with: ReplayCompatibility,
            attachments: Vec<BlockReplayWire>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct BlockReplayWire {
            block: u16,
            payload: String,
        }

        let wire = Wire::deserialize(deserializer)?;
        let attachments = wire
            .attachments
            .into_iter()
            .map(|item| BlockReplay {
                block: item.block,
                payload: item.payload,
            })
            .collect();
        Self::new(wire.compatible_with, attachments).map_err(serde::de::Error::custom)
    }
}

/// One complete model output with ordered semantics and separate private replay.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssistantOutput {
    blocks: Box<[AssistantBlock]>,
    replay: Option<AssistantReplay>,
}

impl AssistantOutput {
    pub fn new(
        blocks: Vec<AssistantBlock>,
        replay: Option<AssistantReplay>,
    ) -> Result<Self, ContextError> {
        if blocks.is_empty() {
            return Err(ContextError::EmptyAssistantOutput);
        }
        if blocks.len() > usize::from(u16::MAX) {
            return Err(ContextError::TooManyAssistantBlocks);
        }
        let mut item_ids = BTreeSet::new();
        let mut call_ids = BTreeSet::new();
        let mut text_bytes = 0_usize;
        let mut tool_argument_bytes = 0_usize;
        for block in &blocks {
            if !item_ids.insert(block.item_id().clone()) {
                return Err(ContextError::DuplicateAssistantItemId);
            }
            if matches!(block, AssistantBlock::Text { text, .. } if text.is_empty()) {
                return Err(ContextError::EmptyTextBlock);
            }
            if let AssistantBlock::Text { text, .. } | AssistantBlock::Reasoning { text, .. } =
                block
            {
                text_bytes = text_bytes
                    .checked_add(text.len())
                    .ok_or(ContextError::AssistantTextTooLarge)?;
                if text_bytes > MAX_ASSISTANT_TEXT_BYTES {
                    return Err(ContextError::AssistantTextTooLarge);
                }
            }
            if let AssistantBlock::ToolCall { call, .. } = block {
                if call.call_id.as_str().len() > MAX_TOOL_IDENTITY_BYTES
                    || call.name.len() > MAX_TOOL_IDENTITY_BYTES
                {
                    return Err(ContextError::ToolIdentityTooLarge);
                }
                if call.arguments.len() > crate::MAX_REQUESTED_TOOL_ARGUMENT_BYTES {
                    return Err(ContextError::ToolArgumentsTooLarge);
                }
                if !call_ids.insert(call.call_id.clone()) {
                    return Err(ContextError::DuplicateToolCallId);
                }
                tool_argument_bytes = tool_argument_bytes
                    .checked_add(call.arguments.len())
                    .ok_or(ContextError::ToolArgumentsTooLarge)?;
                if tool_argument_bytes > MAX_ASSISTANT_TOOL_ARGUMENT_BYTES {
                    return Err(ContextError::ToolArgumentsTooLarge);
                }
            }
        }
        if let Some(replay) = &replay {
            for attachment in replay.attachments() {
                blocks
                    .get(usize::from(attachment.block()))
                    .ok_or(ContextError::ReplayAnchorOutOfRange)?;
            }
        }
        for (index, block) in blocks.iter().enumerate() {
            if matches!(block, AssistantBlock::ReplayOnly { .. })
                && replay.as_ref().is_none_or(|replay| {
                    !replay
                        .attachments()
                        .iter()
                        .any(|item| usize::from(item.block()) == index)
                })
            {
                return Err(ContextError::MissingReplay);
            }
            if let AssistantBlock::Reasoning { text, .. } = block
                && text.is_empty()
                && replay.as_ref().is_none_or(|replay| {
                    !replay
                        .attachments()
                        .iter()
                        .any(|item| usize::from(item.block()) == index)
                })
            {
                return Err(ContextError::EmptyReasoningBlock);
            }
        }
        Ok(Self {
            blocks: blocks.into_boxed_slice(),
            replay,
        })
    }

    #[must_use]
    pub fn blocks(&self) -> &[AssistantBlock] {
        &self.blocks
    }

    #[must_use]
    pub const fn replay(&self) -> Option<&AssistantReplay> {
        self.replay.as_ref()
    }

    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.blocks.iter().filter_map(AssistantBlock::tool_call)
    }
}

impl<'de> Deserialize<'de> for AssistantOutput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            blocks: Vec<AssistantBlock>,
            replay: Option<AssistantReplay>,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.blocks, wire.replay).map_err(serde::de::Error::custom)
    }
}

/// One terminal result paired to its call by identity and declaration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolBatchResult {
    call_id: ToolCallId,
    outcome: ToolOutcome,
}

impl ToolBatchResult {
    #[must_use]
    pub const fn new(call_id: ToolCallId, outcome: ToolOutcome) -> Self {
        Self { call_id, outcome }
    }

    #[must_use]
    pub const fn call_id(&self) -> &ToolCallId {
        &self.call_id
    }

    #[must_use]
    pub const fn outcome(&self) -> &ToolOutcome {
        &self.outcome
    }
}

/// One indivisible assistant output and all tool results it requested.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolBatch {
    assistant: AssistantOutput,
    results: Box<[ToolBatchResult]>,
}

impl ToolBatch {
    pub fn new(
        assistant: AssistantOutput,
        results: Vec<ToolBatchResult>,
    ) -> Result<Self, ContextError> {
        let calls: Vec<_> = assistant.tool_calls().collect();
        if calls.is_empty() {
            return Err(ContextError::ToolBatchWithoutCalls);
        }
        if calls.len() != results.len()
            || calls
                .iter()
                .zip(&results)
                .any(|(call, result)| call.call_id != result.call_id)
        {
            return Err(ContextError::ToolResultOrderMismatch);
        }
        if results.iter().any(|result| {
            let retained = match &result.outcome {
                ToolOutcome::Succeeded { output } => output.len(),
                ToolOutcome::Failed { message } => message.len(),
                ToolOutcome::AdmissionRefused { .. }
                | ToolOutcome::PermissionRefused { .. }
                | ToolOutcome::Forbidden
                | ToolOutcome::Denied
                | ToolOutcome::Cancelled { .. } => 0,
            };
            retained > crate::MAX_TOOL_PRESENTATION_TEXT_BYTES
        }) {
            return Err(ContextError::ToolOutcomeTooLarge);
        }
        Ok(Self {
            assistant,
            results: results.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn assistant(&self) -> &AssistantOutput {
        &self.assistant
    }

    #[must_use]
    pub fn results(&self) -> &[ToolBatchResult] {
        &self.results
    }
}

/// One provider-safe context unit that compaction and rewind never split.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextAtom {
    source_entries: Box<[ConversationEntryId]>,
    value: ContextAtomValue,
}

/// Semantic value retained by one context atom.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextAtomValue {
    User { text: String },
    Skill(SkillActivation),
    CompactionSummary { text: String },
    Assistant(AssistantOutput),
    ToolBatch(ToolBatch),
}

impl ContextAtom {
    pub fn user(source: ConversationEntryId, text: String) -> Self {
        Self {
            source_entries: vec![source].into_boxed_slice(),
            value: ContextAtomValue::User { text },
        }
    }

    /// Retains one explicit skill activation separately from user-authored text (SKL-5).
    pub fn skill(source: ConversationEntryId, activation: SkillActivation) -> Self {
        Self {
            source_entries: vec![source].into_boxed_slice(),
            value: ContextAtomValue::Skill(activation),
        }
    }

    /// Creates the harness-supplied context base for one durable checkpoint (CPL-3).
    #[must_use]
    pub fn compaction_summary(source: ConversationEntryId, text: String) -> Self {
        Self {
            source_entries: vec![source].into_boxed_slice(),
            value: ContextAtomValue::CompactionSummary { text },
        }
    }

    pub fn assistant(
        source: ConversationEntryId,
        output: AssistantOutput,
    ) -> Result<Self, ContextError> {
        if output.tool_calls().next().is_some() {
            return Err(ContextError::AssistantAtomContainsCalls);
        }
        Ok(Self {
            source_entries: vec![source].into_boxed_slice(),
            value: ContextAtomValue::Assistant(output),
        })
    }

    pub fn tool_batch(
        source_entries: Vec<ConversationEntryId>,
        batch: ToolBatch,
    ) -> Result<Self, ContextError> {
        if source_entries.is_empty() {
            return Err(ContextError::EmptySourceEntries);
        }
        let mut unique = BTreeSet::new();
        if source_entries
            .iter()
            .any(|entry| !unique.insert(entry.clone()))
        {
            return Err(ContextError::DuplicateSourceEntry);
        }
        Ok(Self {
            source_entries: source_entries.into_boxed_slice(),
            value: ContextAtomValue::ToolBatch(batch),
        })
    }

    #[must_use]
    pub fn source_entries(&self) -> &[ConversationEntryId] {
        &self.source_entries
    }

    #[must_use]
    pub const fn value(&self) -> &ContextAtomValue {
        &self.value
    }
}
