//! Immutable artifact origins used by authenticated cross-session references.

use plexmaton_core::{AgentId, ArtifactId, ConversationEntryId, ConversationId, HeadName};

use super::{ConversationJournal, JournalEntryPayload, JournalError, JournalSequence};

/// Exact retained journal fact from which an owner may issue an opaque artifact selector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactAnnouncementOrigin {
    conversation: ConversationId,
    agent: AgentId,
    entry: ConversationEntryId,
    sequence: JournalSequence,
    artifact: ArtifactId,
}

impl ArtifactAnnouncementOrigin {
    #[must_use]
    pub const fn conversation(&self) -> &ConversationId {
        &self.conversation
    }

    #[must_use]
    pub const fn agent(&self) -> &AgentId {
        &self.agent
    }

    #[must_use]
    pub const fn entry(&self) -> &ConversationEntryId {
        &self.entry
    }

    #[must_use]
    pub const fn sequence(&self) -> JournalSequence {
        self.sequence
    }

    #[must_use]
    pub const fn artifact(&self) -> &ArtifactId {
        &self.artifact
    }
}

impl ConversationJournal {
    /// Artifact facts reachable from one selected branch, in provider order.
    pub fn artifact_origins_on(
        &self,
        head: &HeadName,
    ) -> Result<Vec<ArtifactAnnouncementOrigin>, JournalError> {
        self.path(head)?
            .into_iter()
            .filter_map(|entry| self.artifact_origin(entry))
            .collect::<Result<Vec<_>, _>>()
    }

    /// Every retained artifact fact in append order, including abandoned branches.
    pub fn retained_artifact_origins(
        &self,
    ) -> Result<Vec<ArtifactAnnouncementOrigin>, JournalError> {
        self.records()
            .iter()
            .filter_map(|record| match record {
                super::JournalRecord::AppendEntry { entry, .. } => self.artifact_origin(entry),
                _ => None,
            })
            .collect()
    }

    fn artifact_origin(
        &self,
        entry: &super::ConversationEntry,
    ) -> Option<Result<ArtifactAnnouncementOrigin, JournalError>> {
        let JournalEntryPayload::ArtifactAnnounced {
            agent_id,
            artifact_id,
            ..
        } = &entry.payload
        else {
            return None;
        };
        Some(
            self.entry_sequences
                .get(&entry.id)
                .copied()
                .ok_or_else(|| JournalError::MissingEntry(entry.id.clone()))
                .map(|sequence| ArtifactAnnouncementOrigin {
                    conversation: self.conversation_id().clone(),
                    agent: agent_id.clone(),
                    entry: entry.id.clone(),
                    sequence,
                    artifact: artifact_id.clone(),
                }),
        )
    }
}
