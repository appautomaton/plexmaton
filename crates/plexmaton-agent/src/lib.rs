//! The agent loop: what a turn does next, decided without doing any of it.
//!
//! This crate holds the machine and nothing else. It has no asynchronous runtime, no HTTP client,
//! no filesystem access and no terminal, and its manifest is where that is enforced: an effect is
//! a value returned to a caller who owns the outside world. That is what makes a turn testable
//! from a script, inspectable between any two inputs, and — when Phase 02 arrives — resumable
//! from its canonical journal.
//!
//! The vocabulary crossing outward is [`plexmaton_core::SessionEvent`]; the vocabulary crossing
//! inward is [`Input`]. They never merge.

mod admission;
mod interface;
mod journal;
mod model;
mod record;
mod step;
mod timing;
mod tools;
mod turn;

#[cfg(test)]
mod test_support;

pub use admission::{
    AdmissionOutcome, AdmissionRefusal, AdmissionRequest, AdmittedCallError, AdmittedToolCall,
    ApprovalPolicy, CapabilitySet, MAX_ADMITTED_ARGUMENT_BYTES, MAX_APPROVAL_DETAIL_BYTES,
    MAX_REQUESTED_TOOL_ARGUMENT_BYTES, PolicyDecision, ToolDefinitionRevision,
};
pub use interface::{
    ApprovalDecisionRefusal, Effect, Input, ModelDeliveryRefusal, Reaction, ReleasedInput,
    UndeliveredInput, UndeliveredModelInput, UndeliveredReason, UnresolvedApprovalDecision,
};
pub use journal::{
    HeadRevision, JournalEntryPayload, JournalError, JournalProjection, JournalProjectionError,
    JournalRecord, JournalSequence, RecoveryProjection, SessionEntry, SessionJournal,
    SessionMetadata,
};
pub use model::{
    AssistantBlock, AssistantOutput, AssistantReplay, BlockReplay, ContextAtom, ContextAtomValue,
    ContextError, MAX_ASSISTANT_TEXT_BYTES, MAX_ASSISTANT_TOOL_ARGUMENT_BYTES,
    MAX_PROVIDER_REPLAY_BYTES, MAX_TOOL_IDENTITY_BYTES, ModelCall, ModelError, ModelEvent,
    ModelOutputPosition, ModelRequest, ModelStepId, ProviderCodecId, ProviderCodecRevision,
    ProviderModelFamilyId, ProviderReplay, ProviderReplayError, ProviderReplayOwnerId,
    ReplayCompatibility, StopReason, ToolBatch, ToolBatchResult,
};
pub use timing::{ActiveTurnStatus, TurnFinished, TurnFinishedAt, TurnOutcome, UnixMillis};
pub use tools::{
    MAX_TOOL_PRESENTATION_TEXT_BYTES, PendingApproval, ToolCall, ToolCancellationReason,
    ToolExecutionResult, ToolOutcome, bounded_tool_text,
};
pub use turn::{Agent, ProjectionRebuildError, TurnBudget};
