use plexmaton_core::TreeOrigin;

use crate::{JournalError, JournalSequence};

/// A prepared metadata change; runtime acknowledgement gates its publication (TRE-4/TRE-8).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeEditResult {
    /// Origin after the edit, including a renamed selected head and the new journal sequence.
    pub origin: TreeOrigin,
    /// Exactly one mutation, or no mutation when the requested metadata was already equal.
    pub mutation_sequence: Option<JournalSequence>,
}

/// Why a metadata request left the journal unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreeEditRefusal {
    /// Metadata edits do not cancel an active turn or drain queued input.
    Busy,
    /// The request did not address the exact current conversation, agent, head and revision.
    StaleOrigin,
    /// Blank sessions remain unmaterialized until accepted input supplies semantic history.
    EmptyTree,
    /// User-entered branch names share the bounded single-line annotation limit.
    InvalidHeadName,
    /// Only semantic rows visible in this agent's current all-head tree can be annotated.
    EntryUnavailable,
    /// The canonical journal refused a missing, stale or conflicting metadata mutation.
    Journal(JournalError),
}

impl std::fmt::Display for TreeEditRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => {
                formatter.write_str("Wait for the current turn to finish before editing the tree.")
            }
            Self::StaleOrigin => {
                formatter.write_str("History changed. Refresh the tree before editing it.")
            }
            Self::EmptyTree => formatter.write_str("There is no conversation history to edit yet."),
            Self::InvalidHeadName => formatter.write_str(
                "A branch name must fit in 256 UTF-8 bytes and contain no control characters.",
            ),
            Self::EntryUnavailable => {
                formatter.write_str("This entry is no longer available in the conversation tree.")
            }
            Self::Journal(JournalError::CannotAbandonSelectedHead(_)) => {
                formatter.write_str("Select another branch before abandoning the current branch.")
            }
            Self::Journal(JournalError::UnavailableHeadName(_)) => formatter.write_str(
                "That branch name is already in use or was retired. Choose another name.",
            ),
            Self::Journal(JournalError::MissingHead(_)) => {
                formatter.write_str("This branch is no longer available. Refresh the tree.")
            }
            Self::Journal(_) => {
                formatter.write_str("History could not accept this edit safely. Refresh the tree.")
            }
        }
    }
}

impl std::error::Error for TreeEditRefusal {}
