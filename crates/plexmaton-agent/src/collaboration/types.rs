use plexmaton_core::{
    AgentId, ArtifactId, CollaborationItemId, ConversationId, DelegationId, MailId,
};
use serde::{Deserialize, Serialize};

use super::CollaborationError;

/// Maximum UTF-8 bytes in each summary, task or objection; no truncation is performed.
pub const MAX_COLLABORATION_TEXT_BYTES: usize = 32 * 1024;
/// Maximum bytes in identities admitted to the collaboration domain.
pub const MAX_COLLABORATION_ID_BYTES: usize = 256;
/// Maximum distinct durable artifact pointers in one mail item.
pub const MAX_MAIL_ARTIFACTS: usize = 16;
/// Hard retained-item ceiling, including control facts and retry identities.
pub const MAX_COLLABORATION_ITEMS: usize = 4096;
/// Hard ceiling on retained delegation records, independent of mail volume.
pub const MAX_DELEGATIONS: usize = 256;
/// Maximum aggregate semantic mail bytes, excluding serialization and index overhead.
pub const MAX_RETAINED_MAIL_BYTES: usize = 16 * 1024 * 1024;

/// Exact semantic text; bounds apply equally to constructors and persisted decoding (COL-2).
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct CollaborationText(String);

impl CollaborationText {
    /// Validates exact source text before it can enter an event or a decoded record.
    pub fn new(value: impl Into<String>) -> Result<Self, CollaborationError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(CollaborationError::EmptyText);
        }
        if value.len() > MAX_COLLABORATION_TEXT_BYTES {
            return Err(CollaborationError::TextTooLarge);
        }
        Ok(Self(value))
    }

    /// Original UTF-8 content, suitable for a semantic projection without reconstruction.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for CollaborationText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CollaborationText")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl TryFrom<String> for CollaborationText {
    type Error = CollaborationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CollaborationText> for String {
    fn from(value: CollaborationText) -> Self {
        value.0
    }
}

/// Session identity disambiguates agent names that are local to a conversation.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MailEndpoint {
    pub conversation: ConversationId,
    pub agent: AgentId,
}

impl MailEndpoint {
    pub(super) fn validate(&self) -> Result<(), CollaborationError> {
        validate_id(self.conversation.as_str())?;
        validate_id(self.agent.as_str())
    }

    pub(super) fn bytes(&self) -> usize {
        self.conversation.as_str().len() + self.agent.as_str().len()
    }
}

/// An artifact remains in its owning conversation; mail carries only this reference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReference {
    pub conversation: ConversationId,
    pub artifact: ArtifactId,
}

/// Immutable accepted mail. The ledger validates endpoint, pointer and aggregate bounds.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MailEnvelope {
    /// Sender-scoped identity, independent of the containing collaboration item identity.
    pub id: MailId,
    pub from: MailEndpoint,
    pub to: MailEndpoint,
    pub summary: CollaborationText,
    /// Distinct pointers only; the runtime must separately establish artifact accessibility.
    pub artifacts: Vec<ArtifactReference>,
}

impl MailEnvelope {
    pub(super) fn validate(&self) -> Result<(), CollaborationError> {
        validate_id(self.id.as_str())?;
        self.from.validate()?;
        self.to.validate()?;
        if self.from.conversation == self.to.conversation {
            return Err(CollaborationError::SameSession);
        }
        if self.artifacts.len() > MAX_MAIL_ARTIFACTS {
            return Err(CollaborationError::TooManyArtifacts);
        }
        let mut seen = std::collections::BTreeSet::new();
        for pointer in &self.artifacts {
            if !seen.insert((&pointer.conversation, &pointer.artifact)) {
                return Err(CollaborationError::DuplicateArtifact);
            }
            validate_id(pointer.conversation.as_str())?;
            validate_id(pointer.artifact.as_str())?;
        }
        Ok(())
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.id.as_str().len()
            + self.from.bytes()
            + self.to.bytes()
            + self.summary.as_str().len()
            + self
                .artifacts
                .iter()
                .map(|pointer| {
                    pointer.conversation.as_str().len() + pointer.artifact.as_str().len()
                })
                .sum::<usize>()
    }
}

/// Attribution supplied by a trusted runtime; serialized authors do not authenticate themselves.
/// The reducer checks agent ownership and user precedence under COL-3.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "endpoint",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DelegationAuthor {
    User,
    Agent(MailEndpoint),
}

/// Task revision, distinct from the sequence of items in the containing collaboration.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DelegationRevision(pub u64);

/// Contiguous position in one collaboration log; the reducer validates one-based ordering.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CollaborationSequence(pub u64);

/// Collaboration facts only: no provider wire events, terminal input, or UI animation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CollaborationEvent {
    /// Accepted cross-session mail; inclusion in a provider request is a separate future fact.
    MailAccepted { mail: MailEnvelope },
    /// Fixes endpoints and the initial task; worker assignment and ancestry must be consistent.
    DelegationCreated {
        delegation: DelegationId,
        delegator: MailEndpoint,
        worker: MailEndpoint,
        task: CollaborationText,
    },
    /// Replaces the effective task under COL-3; `expected` is the author's observed task revision.
    TaskAmended {
        delegation: DelegationId,
        expected: DelegationRevision,
        author: DelegationAuthor,
        task: CollaborationText,
    },
    /// Records a delegator objection without advancing task revision or changing the task.
    ObjectionRaised {
        delegation: DelegationId,
        revision: DelegationRevision,
        author: MailEndpoint,
        summary: CollaborationText,
    },
}

/// One immutable item with its exact retry identity and expected sequence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationRecord {
    pub id: CollaborationItemId,
    pub sequence: CollaborationSequence,
    pub event: CollaborationEvent,
}

/// Identifies the original accepted item, including for an exact retry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemReceipt {
    pub id: CollaborationItemId,
    pub sequence: CollaborationSequence,
}

impl CollaborationRecord {
    /// Refers to this immutable item; constructing a receipt alone proves no disk acknowledgement.
    #[must_use]
    pub fn receipt(&self) -> ItemReceipt {
        ItemReceipt {
            id: self.id.clone(),
            sequence: self.sequence,
        }
    }
}

/// Immutable per-file retention policy; bounds cannot expand during replay.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationLimits {
    /// Total retained items, including mail, task edits and objections.
    pub items: usize,
    /// Retained delegation count; this is not a concurrent runner limit.
    pub delegations: usize,
    /// Sum of admitted mail text, endpoint identities and artifact-pointer identities.
    pub mail_bytes: usize,
    /// Tail slots unavailable to ordinary mail, leaving room for task control facts.
    pub control_items: usize,
}

impl Default for CollaborationLimits {
    fn default() -> Self {
        Self {
            items: MAX_COLLABORATION_ITEMS,
            delegations: MAX_DELEGATIONS,
            mail_bytes: MAX_RETAINED_MAIL_BYTES,
            control_items: 256,
        }
    }
}

impl CollaborationLimits {
    pub(super) fn validate(self) -> Result<(), CollaborationError> {
        if self.items == 0
            || self.items > MAX_COLLABORATION_ITEMS
            || self.delegations == 0
            || self.delegations > MAX_DELEGATIONS
            || self.mail_bytes == 0
            || self.mail_bytes > MAX_RETAINED_MAIL_BYTES
            || self.control_items > self.items
        {
            return Err(CollaborationError::InvalidLimits);
        }
        Ok(())
    }
}

/// Reconstructed effective task; amendments and objections remain in the canonical item log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegationView {
    pub delegator: MailEndpoint,
    pub worker: MailEndpoint,
    pub task: CollaborationText,
    pub revision: DelegationRevision,
    pub author: DelegationAuthor,
    pub(super) last_user_revision: Option<DelegationRevision>,
}

pub(super) fn validate_id(value: &str) -> Result<(), CollaborationError> {
    if value.len() > MAX_COLLABORATION_ID_BYTES {
        return Err(CollaborationError::IdentityTooLarge);
    }
    Ok(())
}
