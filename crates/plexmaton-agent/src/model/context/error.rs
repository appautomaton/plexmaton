//! Validation failures shared by canonical blocks, replay and tool batches.

/// Invalid semantic context shape refused before provider encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextError {
    EmptyAssistantOutput,
    TooManyAssistantBlocks,
    DuplicateAssistantItemId,
    DuplicateToolCallId,
    AssistantTextTooLarge,
    ToolIdentityTooLarge,
    ToolArgumentsTooLarge,
    EmptyTextBlock,
    EmptyReasoningBlock,
    EmptyReplay,
    EmptyReplayPayload,
    UnorderedReplayAnchor,
    MixedReplayCompatibility,
    ReplayTooLarge,
    ReplayAnchorOutOfRange,
    MissingReplay,
    AssistantAtomContainsCalls,
    ToolBatchWithoutCalls,
    ToolResultOrderMismatch,
    ToolOutcomeTooLarge,
    EmptySourceEntries,
    DuplicateSourceEntry,
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptyAssistantOutput => "assistant output has no semantic or replay block",
            Self::TooManyAssistantBlocks => "assistant output exceeds its block ordinal space",
            Self::DuplicateAssistantItemId => "assistant output repeats a transcript item identity",
            Self::DuplicateToolCallId => "assistant output repeats a tool call identity",
            Self::AssistantTextTooLarge => "assistant output exceeds its semantic text byte bound",
            Self::ToolIdentityTooLarge => "assistant tool identity exceeds its byte bound",
            Self::ToolArgumentsTooLarge => {
                "assistant output exceeds its aggregate tool-argument byte bound"
            }
            Self::EmptyTextBlock => "assistant text block is empty",
            Self::EmptyReasoningBlock => {
                "assistant reasoning block is empty and has no replay attachment"
            }
            Self::EmptyReplay => "assistant replay has no attachment",
            Self::EmptyReplayPayload => "assistant replay attachment payload is empty",
            Self::UnorderedReplayAnchor => "assistant replay attachments are not strictly ordered",
            Self::MixedReplayCompatibility => {
                "assistant replay attachments have mixed compatibility"
            }
            Self::ReplayTooLarge => "assistant replay exceeds its total byte bound",
            Self::ReplayAnchorOutOfRange => "assistant replay anchor is outside the block sequence",
            Self::MissingReplay => "a replay-only block has no replay attachment",
            Self::AssistantAtomContainsCalls => "assistant atom contains a tool call",
            Self::ToolBatchWithoutCalls => "tool batch has no calls",
            Self::ToolResultOrderMismatch => "tool results do not match model call order",
            Self::ToolOutcomeTooLarge => "tool result exceeds its retained model-output byte bound",
            Self::EmptySourceEntries => "context atom has no source entry",
            Self::DuplicateSourceEntry => "context atom repeats a source entry identity",
        };
        formatter.write_str(message)
    }
}
