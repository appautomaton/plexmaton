use crate::{AgentId, ConversationEntryId, ConversationId, HeadName};

/// Exact journal high-water token used to address one acknowledged conversation-tree snapshot.
///
/// The agent derives this from the journal's next expected sequence. It changes after every
/// accepted record, including audit and terminal records that do not move a head revision.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TreeRevision(u64);

impl TreeRevision {
    /// Creates a revision token from the journal's next expected sequence.
    #[must_use]
    pub const fn new(next_sequence: u64) -> Self {
        Self(next_sequence)
    }

    /// Numeric high-water token supplied by the agent journal.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Ownership and revision of one tree snapshot read from acknowledged agent state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeOrigin {
    /// Conversation whose journal supplied the snapshot.
    pub conversation_id: ConversationId,
    /// Agent whose transcript and navigation targets are displayed.
    pub agent_id: AgentId,
    /// Selected source head when the snapshot was read.
    pub selected_head: HeadName,
    /// Exact journal high-water token when the snapshot was read.
    pub revision: TreeRevision,
}

/// One navigation request addressed to the exact snapshot from which it was chosen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeNavigation {
    /// Snapshot ownership and sequence fence the agent must revalidate.
    pub origin: TreeOrigin,
    /// Stable semantic destination requested by the caller.
    pub target: TreeNavigationTarget,
}

/// Typed destination for a conversation-tree navigation request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreeNavigationTarget {
    /// Fork a fresh selected head before a user turn or after a completed assistant turn.
    Rewind(ConversationEntryId),
    /// Select an existing named head without creating conversation content.
    SelectHead(HeadName),
}
