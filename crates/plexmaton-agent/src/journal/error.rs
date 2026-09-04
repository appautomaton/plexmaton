use plexmaton_core::{HeadName, JournalRecordId, SessionEntryId};

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
}
