use crate::{ConversationEntryId, HeadName, TreeLabel, TreeOrigin};

/// One bounded semantic view of every currently named head in a conversation journal.
///
/// A successfully built snapshot is complete. The agent returns a typed limit error rather than
/// exposing a prefix that could be mistaken for the whole tree (TRE-2).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeSnapshot {
    /// Exact conversation, agent, selected-head, and journal-sequence origin.
    pub origin: TreeOrigin,
    /// Existing named heads, including heads whose target is the empty root.
    pub heads: Vec<TreeHead>,
    /// Semantic rows in journal chronology, with shared ancestors represented once.
    pub rows: Vec<TreeRow>,
}

/// One named branch pointer in a tree snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeHead {
    /// Stable name of this branch.
    pub name: HeadName,
    /// Stable entry at the branch tip, or `None` for the empty root.
    pub target: Option<ConversationEntryId>,
}

/// One user-visible semantic row in the union of active head ancestry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeRow {
    /// Stable canonical entry represented by this row.
    ///
    /// Assistant output rows use the assistant-output entry even when the row also groups a
    /// complete or in-progress tool batch.
    pub entry_id: ConversationEntryId,
    /// Nearest earlier semantic row on this entry's ancestry, if one exists.
    pub parent_id: Option<ConversationEntryId>,
    /// Zero-based position among emitted rows in chronological journal order.
    pub chronological_ordinal: u64,
    /// Semantic category used for row presentation.
    pub kind: TreeRowKind,
    /// Bounded source preview, with explicit UTF-8-safe truncation state.
    pub preview: TreePreview,
    /// Optional durable annotation attached to this semantic entry (TRE-8).
    pub label: Option<TreeLabel>,
    /// Named heads whose tip falls on this row or one of its grouped journal entries.
    pub head_markers: Vec<HeadName>,
    /// Whether this row belongs to the selected head's ancestry.
    pub active_ancestry: bool,
    /// Rewind eligibility resolved by the journal's canonical navigation resolver.
    pub rewind: TreeRewindEligibility,
}

/// Category of semantic row represented by a tree snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeRowKind {
    /// A user turn that starts a new question.
    User,
    /// User steering attached inside an already-open turn.
    Steering,
    /// One complete assistant output, regardless of how many semantic blocks it contains.
    Assistant,
    /// An assistant output and its tool calls/results as one indivisible row.
    ToolBatch,
    /// A branch-local compaction checkpoint.
    Checkpoint,
    /// A visible system, artifact, collaboration, or mail notice.
    Notice,
}

/// Bounded UTF-8 preview text for one semantic tree row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreePreview {
    /// Source-derived preview text; never contains provider replay attachments.
    pub text: String,
    /// Whether omitted source text is indicated by a trailing Unicode ellipsis.
    pub truncated: bool,
}

/// Typed result of asking whether a semantic tree row is a safe rewind target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeRewindEligibility {
    /// The agent journal resolver accepted this stable entry as a rewind target.
    Eligible,
    /// The agent journal resolver refused this entry as a partial or unsupported target.
    Ineligible,
}

/// Resource bound that stopped construction of a complete tree snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeSnapshotLimit {
    /// Maximum number of active named heads in one snapshot.
    Heads,
    /// Maximum UTF-8 bytes in each retained branch name, including names from older journals.
    HeadNameBytes,
    /// Maximum ancestry entries visited across the deduplicated all-head union.
    AncestryEntries,
    /// Maximum journal records scanned to establish chronology and semantic rows.
    ScannedRecords,
    /// Maximum semantic tree nodes emitted in one snapshot.
    Nodes,
    /// Maximum aggregate UTF-8 preview bytes retained across rows.
    AggregatePreviewBytes,
}

/// A tree snapshot could not be built completely within its named resource bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TreeSnapshotError {
    /// The observed work or retained data exceeded one configured hard bound.
    #[error("conversation tree exceeded {limit:?} limit: {observed} > {maximum}")]
    LimitExceeded {
        /// Bound that was crossed.
        limit: TreeSnapshotLimit,
        /// Maximum accepted value for that bound.
        maximum: usize,
        /// First observed value beyond the maximum.
        observed: usize,
    },
}
