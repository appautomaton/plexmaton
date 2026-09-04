use plexmaton_core::{
    AgentId, HeadName, JournalRecordId, SessionEntryId, TranscriptItemId, TurnId,
};

use super::{HeadRevision, JournalSequence};

/// Why a record was refused without changing journal state (JRN-2).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalError {
    /// The record did not carry the stream's exact next sequence.
    UnexpectedSequence {
        expected: JournalSequence,
        actual: JournalSequence,
    },
    /// A prior record already used this identity.
    DuplicateRecord(JournalRecordId),
    /// A prior entry already used this identity.
    DuplicateEntry(SessionEntryId),
    /// A referenced parent or head target does not exist.
    MissingEntry(SessionEntryId),
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
        expected: Option<SessionEntryId>,
        actual: Option<SessionEntryId>,
    },
    /// No later stream sequence can be represented.
    SequenceExhausted,
    /// No later revision can be represented for this head.
    RevisionExhausted(HeadName),
    /// A second semantic start reused one turn identity.
    DuplicateTurn(TurnId),
    /// Agent creation tried to bypass the idle initial lifecycle boundary.
    InvalidInitialAgentStatus(AgentId),
    /// A generic message tried to bypass the typed user-input boundary.
    TimelessUserMessage(TranscriptItemId),
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
        boundary: SessionEntryId,
    },
    /// Steering named a turn that had already reached its terminal fact.
    ClosedTurnInput(TurnId),
    /// Recovery observation and outcome contradicted one another.
    InvalidTurnFinishTime(TurnId),
    /// A branch operation targeted semantic work whose turn is not terminal there.
    UnstableTurnTarget(TurnId),
}
