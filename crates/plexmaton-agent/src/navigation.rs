use std::fmt;

use plexmaton_core::{
    AgentId, ConversationEntryId, ConversationId, HeadName, TreeRevision, TurnId,
};

use crate::{JournalError, JournalProjectionError, JournalSequence};

/// Exact historical user input returned after rewinding to its user turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReturnedDraft {
    /// Exact text stored for the historical user turn.
    pub text: String,
    /// Explicitly selected historical skill name, including numeric names, when present.
    pub skill_name: Option<String>,
}

/// A navigation transition prepared by the agent for the runtime's journal acknowledgement gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeNavigationResult {
    /// Head selected by the transition.
    pub selected_head: HeadName,
    /// Sequence of the single head mutation, or `None` when the requested head was already selected.
    pub mutation_sequence: Option<JournalSequence>,
    /// Historical draft returned only when rewinding to a user turn.
    pub returned_draft: Option<ReturnedDraft>,
}

/// Why the agent refused to navigate from an addressed tree snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreeNavigationRefusal {
    /// The agent still owns a turn or the selected journal path has an open turn.
    Busy,
    /// The addressed snapshot belongs to another conversation.
    ForeignConversation {
        /// Conversation owned by this agent.
        expected: ConversationId,
        /// Conversation named by the request.
        actual: ConversationId,
    },
    /// The addressed snapshot belongs to another agent.
    ForeignAgent {
        /// Agent owned by this machine.
        expected: AgentId,
        /// Agent named by the request.
        actual: AgentId,
    },
    /// At least one journal record was accepted after the tree snapshot was read.
    StaleOrigin {
        /// Snapshot's exact journal high-water token.
        expected: TreeRevision,
        /// Current journal high-water token.
        actual: TreeRevision,
    },
    /// Durable selection changed since the tree snapshot was read.
    SourceHeadChanged {
        /// Source head named by the snapshot.
        expected: HeadName,
        /// Currently selected source head.
        actual: HeadName,
    },
    /// The source head still points into an unfinished turn.
    SourceTurnOpen(TurnId),
    /// The requested stable entry is absent from this conversation journal.
    MissingTarget(ConversationEntryId),
    /// The requested turn entry belongs to a different agent.
    ForeignTargetAgent {
        /// Agent allowed to navigate this tree.
        expected: AgentId,
        /// Agent that owns the requested entry.
        actual: AgentId,
    },
    /// Steering is an interior event of its turn, not an independent rewind boundary.
    SteeringTarget(ConversationEntryId),
    /// An earlier assistant step is inside its turn and cannot be selected as a standalone boundary.
    InteriorAssistantTarget(ConversationEntryId),
    /// The entry is not an eligible user or assistant navigation target.
    UnsupportedTarget(ConversationEntryId),
    /// An assistant output cannot be targeted until its turn has a terminal journal fact.
    AssistantTurnIncomplete(TurnId),
    /// The assistant output is not on the ancestry of its terminal boundary.
    TargetOutsideTurn {
        /// Assistant output entry that was addressed.
        entry_id: ConversationEntryId,
        /// Turn whose terminal boundary did not include that output.
        turn_id: TurnId,
    },
    /// The requested named destination does not exist.
    MissingHead(HeadName),
    /// Existing destination remains inside an unfinished turn.
    UnstableDestination {
        /// Head whose current target is unstable.
        head: HeadName,
        /// Turn that still owns the destination boundary.
        turn_id: TurnId,
    },
    /// No fresh sequence-derived rewind head name could be represented.
    HeadNameExhausted,
    /// The canonical journal refused the prepared head mutation or target check.
    Journal(JournalError),
    /// The destination ancestry could not be projected into context and visible events.
    Projection(JournalProjectionError),
}

impl fmt::Display for TreeNavigationRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Busy | Self::SourceTurnOpen(_) => {
                "Wait for the current turn and queued input to finish."
            }
            Self::ForeignConversation { .. } => "This tree belongs to a different conversation.",
            Self::ForeignAgent { .. } | Self::ForeignTargetAgent { .. } => {
                "This destination belongs to a different agent."
            }
            Self::StaleOrigin { .. } | Self::SourceHeadChanged { .. } => {
                "History changed. Refresh the tree before navigating."
            }
            Self::MissingTarget(_) => "This message is no longer available in the conversation.",
            Self::SteeringTarget(_) => {
                "Steering belongs to its turn. Choose that turn's original message instead."
            }
            Self::InteriorAssistantTarget(_) => {
                "This is an intermediate assistant step. Choose the turn's final assistant response."
            }
            Self::UnsupportedTarget(_) => {
                "This row is informational and cannot be a rewind target."
            }
            Self::AssistantTurnIncomplete(_) => "Wait for this assistant turn to finish.",
            Self::TargetOutsideTurn { .. } => {
                "This message is outside the completed turn's history."
            }
            Self::MissingHead(_) => "This branch is no longer available. Refresh the tree.",
            Self::UnstableDestination { .. } => "This branch still contains an unfinished turn.",
            Self::HeadNameExhausted => "A new branch name could not be allocated.",
            Self::Journal(_) => {
                "History could not accept this navigation safely. Refresh the tree."
            }
            Self::Projection(_) => "This destination's conversation context could not be restored.",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for TreeNavigationRefusal {}
