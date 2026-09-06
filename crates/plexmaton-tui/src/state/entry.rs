//! Typed entries in one agent's ordered transcript projection.

use plexmaton_core::{
    AgentId, ArtifactId, MailId, ToolCallId, ToolCallStatus, ToolPresentation, TranscriptItemId,
    TranscriptRole,
};

use super::ReduceError;

/// Projected transcript content. Semantic source is retained separately from terminal cells.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptItemView {
    pub id: TranscriptItemId,
    pub role: TranscriptRole,
    pub kind: TranscriptTextKind,
    pub source: String,
    pub revision: u64,
    pub finalized: bool,
}

/// Semantic treatment of text-shaped transcript content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptTextKind {
    /// Ordinary user, assistant, reasoning, or system text.
    Message,
    /// A non-fatal producer condition.
    Warning,
    /// A failed producer operation.
    Error,
}

impl TranscriptItemView {
    /// Accepts the next per-item revision, rejecting a lost or duplicated update.
    pub(super) fn advance_revision(&mut self, received: u64) -> Result<(), ReduceError> {
        let expected = self.revision + 1;
        if received != expected {
            return Err(ReduceError::ItemRevisionGap {
                item_id: self.id.clone(),
                expected,
                received,
            });
        }
        self.revision = received;
        Ok(())
    }
}

/// One stable position in an agent's semantic transcript.
///
/// Domain identities inside a variant correlate the entry with its source; only `id()` controls
/// display order and replay updates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranscriptEntryView {
    /// User, assistant, reasoning, or system-authored text.
    Text(TranscriptItemView),
    /// One tool call throughout its lifecycle.
    Tool(ToolCallView),
    /// One announced durable work product.
    Artifact(ArtifactView),
    /// One delivered mail summary.
    Mail(MailView),
}

impl TranscriptEntryView {
    /// Stable first-appearance identity of this entry.
    #[must_use]
    pub fn id(&self) -> &TranscriptItemId {
        match self {
            Self::Text(item) => &item.id,
            Self::Tool(tool) => &tool.entry_id,
            Self::Artifact(artifact) => &artifact.entry_id,
            Self::Mail(mail) => &mail.entry_id,
        }
    }

    /// Revision used by replay validation and later height caching.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        match self {
            Self::Text(item) => item.revision,
            Self::Tool(tool) => tool.revision,
            Self::Artifact(artifact) => artifact.revision,
            Self::Mail(mail) => mail.revision,
        }
    }
}

/// One visible tool call and its current lifecycle state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallView {
    /// Producer receipt shown beside this call; absent from semantic copy and replay.
    pub saved_project_permission: Option<plexmaton_core::PermissionGrantId>,
    pub entry_id: TranscriptItemId,
    pub id: ToolCallId,
    pub label: String,
    pub status: ToolCallStatus,
    pub presentation: ToolPresentation,
    pub revision: u64,
}

/// Durable work product announced by an agent, referenced by pointer rather than copied inline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactView {
    pub entry_id: TranscriptItemId,
    pub id: ArtifactId,
    pub label: String,
    pub pointer: String,
    pub revision: u64,
}

/// Typed mail delivered between sessions.
///
/// Sender identity is part of the product contract, so it is retained rather than reduced away.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailView {
    pub entry_id: TranscriptItemId,
    pub id: MailId,
    pub from: AgentId,
    pub to: AgentId,
    pub summary: String,
    pub revision: u64,
}

impl TranscriptEntryView {
    pub(crate) fn saved_project_permission(&self) -> Option<&plexmaton_core::PermissionGrantId> {
        match self {
            Self::Tool(tool) => tool.saved_project_permission.as_ref(),
            _ => None,
        }
    }
}
