use crate::{ConversationEntryId, TreeOrigin};

/// Maximum exact tree-copy payload; over-limit source is refused, never truncated (TRE-8).
pub const MAX_TREE_SOURCE_BYTES: usize = 8 * 1024 * 1024;

/// Exact semantic source addressed by the same immutable origin as the visible tree row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeSourceRequest {
    /// Acknowledged conversation, agent, selected head and journal revision.
    pub origin: TreeOrigin,
    /// Canonical row identity, never a line number or decorated preview.
    pub entry_id: ConversationEntryId,
}

/// Explicit failure of an exact-source request; none of these return partial text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TreeSourceError {
    /// The owner cannot expose an unacknowledged or failed journal.
    #[error("History is awaiting persistence or requires reopening.")]
    HistoryUnavailable,
    /// Copy must not silently switch to a newer tree or another conversation.
    #[error("History changed. Refresh the tree before copying.")]
    StaleOrigin,
    /// The stable ID is absent from the agent's bounded all-head semantic tree.
    #[error("This entry is not available in the current tree.")]
    EntryUnavailable,
    /// Building the visibility projection exceeded a hard resource bound.
    #[error("This conversation exceeds the tree's snapshot limits.")]
    SnapshotLimit,
    /// Exact output cannot exceed the named source limit or available allocation.
    #[error("The complete source does not fit the 8 MiB copy limit.")]
    TooLarge,
    /// A grouped tool batch has not recorded every terminal outcome yet.
    #[error("Wait for the complete tool batch before copying its source.")]
    IncompleteBatch,
    /// A non-text reference or omitted producer source cannot masquerade as complete text.
    #[error("This entry has no complete textual source available to copy.")]
    NoSource,
}
