use plexmaton_core::{AgentId, ConversationEntryId, HeadName, JournalRecordId, TurnId};

use super::{HeadRevision, JournalSequence};
use crate::{CompactionPlanError, ModelStepId, RequestAttemptId, RequestTimingError};

/// Why a record was refused without changing journal state (JRN-2).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalError {
    /// A collaboration reference or its session boundary was invalid.
    Collaboration(crate::collaboration::CollaborationError),
    /// The selected tail no longer identifies a safe unanswered rate-limited execution.
    RetryUnavailable,
    /// The record did not carry the stream's exact next sequence.
    UnexpectedSequence {
        expected: JournalSequence,
        actual: JournalSequence,
    },
    /// A prior record already used this identity.
    DuplicateRecord(JournalRecordId),
    /// A prior entry already used this identity.
    DuplicateEntry(ConversationEntryId),
    /// A referenced parent or head target does not exist.
    MissingEntry(ConversationEntryId),
    /// The named head does not exist.
    MissingHead(HeadName),
    /// The head name is active or was retired and may not be reused.
    UnavailableHeadName(HeadName),
    /// The compare-and-set revision is stale.
    StaleHead {
        head: HeadName,
        expected: HeadRevision,
        actual: HeadRevision,
    },
    /// An append was prepared for a parent other than the head's current target.
    ParentMismatch {
        head: HeadName,
        expected: Option<ConversationEntryId>,
        actual: Option<ConversationEntryId>,
    },
    /// No later stream sequence can be represented.
    SequenceExhausted,
    /// No later revision can be represented for this head.
    RevisionExhausted(HeadName),
    /// A second semantic start reused one turn identity.
    DuplicateTurn(TurnId),
    /// A second assistant output reused one model-step identity.
    DuplicateModelStep(ModelStepId),
    /// A model step skipped or rewound its one-based position within a turn.
    UnexpectedModelStep {
        turn_id: TurnId,
        expected: u16,
        actual: u16,
    },
    /// No later model-step position can be represented for a turn.
    ModelStepSequenceExhausted(TurnId),
    /// A second authorization reused one request-attempt identity.
    DuplicateRequestAttempt(RequestAttemptId),
    /// An owner still has an earlier request attempt without a terminal fact.
    RequestAttemptOwnerActive(RequestAttemptId),
    /// A terminal fact named no prior request authorization.
    MissingRequestAttempt(RequestAttemptId),
    /// A second terminal fact tried to finish one request attempt again.
    DuplicateRequestAttemptTerminal(RequestAttemptId),
    /// A terminal attempt carried internally inconsistent timing or provider usage.
    InvalidRequestAttemptTerminal {
        attempt_id: RequestAttemptId,
        error: RequestTimingError,
    },
    /// Request authorization did not name the selected head's exact semantic boundary.
    InvalidRequestAttemptBoundary {
        attempt_id: RequestAttemptId,
        boundary: ConversationEntryId,
    },
    /// A collected compaction result failed its bounded semantic validation.
    InvalidCompactionAttempt {
        attempt_id: RequestAttemptId,
        error: CompactionPlanError,
    },
    /// A compaction result named an attempt owned by an ordinary agent step.
    RequestAttemptIsNotCompaction(RequestAttemptId),
    /// A checkpoint named no collected compaction result.
    MissingCompactionAttempt(RequestAttemptId),
    /// The source head, revision, boundary or epoch changed after planning.
    CompactionSourceChanged,
    /// A checkpoint's attempt owner does not match its plan identity.
    CompactionOwnerMismatch(RequestAttemptId),
    /// A checkpoint's attempt environment does not match its plan.
    CompactionEnvironmentMismatch(RequestAttemptId),
    /// A checkpoint named a failed rather than publishable result.
    CompactionAttemptFailed(RequestAttemptId),
    /// A compaction cut did not resolve to one exact projected atom prefix and suffix.
    InvalidCompactionCut,
    /// A completed summary exceeded the plan's stricter byte allowance.
    CompactionSummaryExceedsPlan,
    /// A checkpoint covered no semantic atom beyond the exact user it pins.
    CompactionMakesNoProgress,
    /// A checkpoint attributed its context mutation to an agent absent from that ancestry.
    MissingCompactionAgent(AgentId),
    /// Compaction requires at least one semantic atom on the selected head.
    EmptyCompactionSource(HeadName),
    /// Agent creation tried to bypass the idle initial lifecycle boundary.
    InvalidInitialAgentStatus(AgentId),
    /// A terminal fact named no semantic turn start.
    MissingTurn(TurnId),
    /// A terminal fact moved a turn between agent owners.
    WrongTurnAgent {
        turn_id: TurnId,
        expected: AgentId,
        actual: AgentId,
    },
    /// A second terminal fact tried to finish one turn again.
    DuplicateTurnFinish(TurnId),
    /// A terminal fact named a boundary outside its turn ancestry.
    InvalidTurnBoundary {
        turn_id: TurnId,
        boundary: ConversationEntryId,
    },
    /// Steering named a turn that had already reached its terminal fact.
    ClosedTurnInput(TurnId),
    /// A skill activation did not immediately follow user input for its named turn.
    InvalidSkillActivationOrder(TurnId),
    /// Recovery observation and outcome contradicted one another.
    InvalidTurnFinishTime(TurnId),
    /// A branch operation targeted semantic work whose turn is not terminal there.
    UnstableTurnTarget(TurnId),
}
