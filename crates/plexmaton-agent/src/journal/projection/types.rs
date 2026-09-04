use plexmaton_core::{
    AgentId, AttentionId, SessionEventEnvelope, ToolCallId, ToolCallStatus, TranscriptItemId,
    TurnId,
};

use super::super::JournalError;
use crate::ModelRequest;

/// A safe visible/model reconstruction of one selected journal head (JRN-5).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalProjection {
    pub(super) request: ModelRequest,
    pub(super) events: Vec<SessionEventEnvelope>,
    pub(super) recovery: Option<RecoveryProjection>,
}

impl JournalProjection {
    /// Provider-independent request rebuilt from complete model facts.
    #[must_use]
    pub const fn request(&self) -> &ModelRequest {
        &self.request
    }

    /// UI-facing event stream rebuilt from canonical facts.
    #[must_use]
    pub fn events(&self) -> &[SessionEventEnvelope] {
        &self.events
    }

    /// Incomplete final tool batch omitted from the provider request, when present.
    #[must_use]
    pub const fn recovery(&self) -> Option<&RecoveryProjection> {
        self.recovery.as_ref()
    }
}

/// Explicit recovery information for an incomplete final tool batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryProjection {
    pub(super) omitted_batch_calls: Vec<ToolCallId>,
}

impl RecoveryProjection {
    /// Calls retained for presentation but omitted from provider input with their whole batch.
    #[must_use]
    pub fn omitted_batch_calls(&self) -> &[ToolCallId] {
        &self.omitted_batch_calls
    }
}

/// Why canonical journal facts could not form one deterministic projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalProjectionError {
    /// The selected journal head did not exist or named missing ancestry.
    Journal(JournalError),
    /// A second request reused one tool-call identity.
    DuplicateToolCall(ToolCallId),
    /// A second agent creation reused an active agent identity.
    DuplicateAgent(AgentId),
    /// A fact named an agent that was never created on this head.
    MissingAgent(AgentId),
    /// Two transcript facts attempted to create the same stable position.
    DuplicateTranscriptItem(TranscriptItemId),
    /// An Attention identity moved between agent owners.
    AttentionOwnerMismatch {
        attention_id: AttentionId,
        expected: AgentId,
        actual: AgentId,
    },
    /// A lifecycle transition named no prior request.
    MissingToolCall(ToolCallId),
    /// A transition moved a call between agent conversations.
    WrongToolAgent(ToolCallId),
    /// A transition skipped or repeated the per-item revision.
    UnexpectedToolRevision {
        call_id: ToolCallId,
        expected: u64,
        actual: u64,
    },
    /// A tool lifecycle transition was not forward and declared.
    InvalidToolTransition {
        call_id: ToolCallId,
        from: ToolCallStatus,
        to: ToolCallStatus,
    },
    /// A non-terminal transition carried a model result.
    PrematureToolOutcome(ToolCallId),
    /// A terminal transition did not carry a model result.
    MissingToolOutcome(ToolCallId),
    /// A terminal lifecycle state disagreed with its typed model result.
    ToolOutcomeMismatch(ToolCallId),
    /// A later tool snapshot contradicted retained invocation or outcome detail.
    ToolPresentationConflict(ToolCallId),
    /// A later model fact followed a tool batch that never reached all of its results.
    IncompleteToolBatchBeforeLaterFact(Vec<ToolCallId>),
    /// A selected timing fact named no selected semantic turn start.
    MissingTurn(TurnId),
    /// A selected timing fact changed the agent that owns its turn.
    WrongTurnAgent(TurnId),
    /// A selected path repeated a stable turn identity.
    DuplicateTurn(TurnId),
    /// More UI events cannot be numbered without repeating an identity.
    EventSequenceExhausted,
}

impl From<JournalError> for JournalProjectionError {
    fn from(error: JournalError) -> Self {
        Self::Journal(error)
    }
}
