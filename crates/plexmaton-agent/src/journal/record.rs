use plexmaton_core::{HeadName, JournalRecordId, SessionEntryId};
use serde::{Deserialize, Serialize};

use super::payload::JournalEntryPayload;

/// Monotonic position of one record in a session journal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct JournalSequence(u64);

impl JournalSequence {
    /// Creates a sequence. A new journal expects one first.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Numeric sequence value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Revision used to compare-and-set one named head.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct HeadRevision(u64);

impl HeadRevision {
    /// Creates a revision. A newly created head begins at zero.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Numeric revision value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One immutable node in the session tree.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionEntry {
    /// Stable identity used by parents and heads.
    pub id: SessionEntryId,
    /// Prior entry on this branch; `None` starts a root.
    pub parent_id: Option<SessionEntryId>,
    /// Typed content retained at this node.
    pub payload: JournalEntryPayload,
}

/// One complete append-only journal mutation (JRN-1).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalRecord {
    /// Append an immutable entry and advance one head in the same mutation.
    AppendEntry {
        /// Position in this session's record stream.
        sequence: JournalSequence,
        /// Stable record identity.
        record_id: JournalRecordId,
        /// Head whose current target must equal `entry.parent_id`.
        head: HeadName,
        /// Compare-and-set revision read when this append was prepared.
        expected_head_revision: HeadRevision,
        /// New immutable entry.
        entry: Box<SessionEntry>,
    },
    /// Create another named pointer at an existing entry or at the empty root.
    CreateHead {
        /// Position in this session's record stream.
        sequence: JournalSequence,
        /// Stable record identity.
        record_id: JournalRecordId,
        /// Fresh name for the new head.
        head: HeadName,
        /// Existing entry to point at, or the empty root.
        at: Option<SessionEntryId>,
    },
    /// Move an existing head without creating conversation content.
    MoveHead {
        /// Position in this session's record stream.
        sequence: JournalSequence,
        /// Stable record identity.
        record_id: JournalRecordId,
        /// Existing head to move.
        head: HeadName,
        /// Compare-and-set revision read when this move was prepared.
        expected_head_revision: HeadRevision,
        /// Existing entry to point at, or the empty root.
        to: Option<SessionEntryId>,
    },
    /// Give an existing head a fresh, never-used name.
    RenameHead {
        /// Position in this session's record stream.
        sequence: JournalSequence,
        /// Stable record identity.
        record_id: JournalRecordId,
        /// Existing head to rename.
        head: HeadName,
        /// Compare-and-set revision read when this rename was prepared.
        expected_head_revision: HeadRevision,
        /// Fresh replacement name.
        renamed: HeadName,
    },
    /// Remove a named pointer while retaining every entry it once reached.
    AbandonHead {
        /// Position in this session's record stream.
        sequence: JournalSequence,
        /// Stable record identity.
        record_id: JournalRecordId,
        /// Existing head to retire.
        head: HeadName,
        /// Compare-and-set revision read when this retirement was prepared.
        expected_head_revision: HeadRevision,
    },
}

impl JournalRecord {
    /// Position this record claims in the session stream.
    #[must_use]
    pub const fn sequence(&self) -> JournalSequence {
        match self {
            Self::AppendEntry { sequence, .. }
            | Self::CreateHead { sequence, .. }
            | Self::MoveHead { sequence, .. }
            | Self::RenameHead { sequence, .. }
            | Self::AbandonHead { sequence, .. } => *sequence,
        }
    }

    pub(super) fn record_id(&self) -> &JournalRecordId {
        match self {
            Self::AppendEntry { record_id, .. }
            | Self::CreateHead { record_id, .. }
            | Self::MoveHead { record_id, .. }
            | Self::RenameHead { record_id, .. }
            | Self::AbandonHead { record_id, .. } => record_id,
        }
    }
}
