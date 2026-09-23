//! Which conversation's projection holds each event, read and re-addressed in one place.

use crate::{AgentId, ConversationEvent};

impl ConversationEvent {
    /// The conversation whose projection holds this fact.
    #[must_use]
    pub fn agent(&self) -> &AgentId {
        match self {
            Self::AgentCreated { agent_id, .. }
            | Self::AgentStatusChanged { agent_id, .. }
            | Self::TurnUsageUpdated { agent_id, .. }
            | Self::TranscriptItemStarted { agent_id, .. }
            | Self::TranscriptDelta { agent_id, .. }
            | Self::TranscriptItemFinalized { agent_id, .. }
            | Self::ToolCallChanged { agent_id, .. }
            | Self::ServerToolStarted { agent_id, .. }
            | Self::ServerToolCalled { agent_id, .. }
            | Self::AttentionRequested { agent_id, .. }
            | Self::AttentionResolved { agent_id, .. }
            | Self::TaskAssigned { agent_id, .. }
            | Self::MailDelivered { agent_id, .. }
            | Self::HandoffCompleted { agent_id, .. }
            | Self::ArtifactAnnounced { agent_id, .. }
            | Self::ContextCompacted { agent_id, .. }
            | Self::CompactionStarted { agent_id }
            | Self::CompactionEnded { agent_id }
            | Self::RuntimeWarning { agent_id, .. }
            | Self::RuntimeError { agent_id, .. } => agent_id,
        }
    }

    /// The conversation whose projection holds this fact, so a reader can re-address it.
    ///
    /// A delegated child numbers its own conversation and names itself by its own agent identity.
    /// A root that shows what that child did is showing it inside the root's projection, under the
    /// name the roster gave it, so every forwarded fact is re-addressed here rather than at each
    /// call site — and a new variant is a compile error until it says which conversation it is in.
    pub fn agent_mut(&mut self) -> &mut AgentId {
        match self {
            Self::AgentCreated { agent_id, .. }
            | Self::AgentStatusChanged { agent_id, .. }
            | Self::TurnUsageUpdated { agent_id, .. }
            | Self::TranscriptItemStarted { agent_id, .. }
            | Self::TranscriptDelta { agent_id, .. }
            | Self::TranscriptItemFinalized { agent_id, .. }
            | Self::ToolCallChanged { agent_id, .. }
            | Self::ServerToolStarted { agent_id, .. }
            | Self::ServerToolCalled { agent_id, .. }
            | Self::AttentionRequested { agent_id, .. }
            | Self::AttentionResolved { agent_id, .. }
            | Self::TaskAssigned { agent_id, .. }
            | Self::MailDelivered { agent_id, .. }
            | Self::HandoffCompleted { agent_id, .. }
            | Self::ArtifactAnnounced { agent_id, .. }
            | Self::ContextCompacted { agent_id, .. }
            | Self::CompactionStarted { agent_id }
            | Self::CompactionEnded { agent_id }
            | Self::RuntimeWarning { agent_id, .. }
            | Self::RuntimeError { agent_id, .. } => agent_id,
        }
    }
}
