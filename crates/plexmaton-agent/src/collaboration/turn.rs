use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use plexmaton_core::{CollaborationId, CollaborationItemId, ConversationEntryId, HeadName, TurnId};
use serde::{Deserialize, Serialize};

use super::types::validate_id;
use super::{
    CollaborationError, CollaborationEvent, CollaborationLedger, CollaborationRecord,
    CollaborationSequence, DelegationRevision, MailEndpoint, Preparation,
};
use crate::HeadRevision;

/// Maximum source facts admitted together; overflow holds the whole turn.
pub const MAX_TURN_SOURCE_ITEMS: usize = 64;
/// Semantic text and variable identity bytes, including authorship, per admitted prefix.
pub const MAX_TURN_SOURCE_BYTES: usize = 256 * 1024;

/// A canonical item reference is scoped to a collaboration, never just a queue position.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationItemRef {
    pub collaboration: CollaborationId,
    pub item: CollaborationItemId,
    pub sequence: CollaborationSequence,
}

impl CollaborationItemRef {
    /// Checks reference bounds before any lookup in a canonical log.
    pub fn validate(&self) -> Result<(), CollaborationError> {
        validate_id(self.collaboration.as_str())?;
        validate_id(self.item.as_str())?;
        if self.sequence.0 == 0 {
            return Err(CollaborationError::InvalidReference);
        }
        Ok(())
    }
}

/// Exact session boundary proposed by the runner before the collaboration append.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnBoundary {
    pub recipient: MailEndpoint,
    pub head: HeadName,
    pub head_revision: HeadRevision,
    pub parent: Option<ConversationEntryId>,
    pub turn: TurnId,
}

/// Frozen eligible prefix. A session's previous included admission supplies its delivery cursor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnAdmission {
    pub boundary: TurnBoundary,
    pub previous: Option<CollaborationItemRef>,
    pub items: Vec<CollaborationItemRef>,
}

/// An immutable source fact and its task revision at that position, not at resolution time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedCollaborationItem {
    pub reference: CollaborationItemRef,
    pub event: CollaborationEvent,
    pub task_revision: Option<DelegationRevision>,
}

/// Materialized context derived only from a validated canonical admission; never serialized.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedTurnAdmission {
    reference: CollaborationItemRef,
    admission: TurnAdmission,
    items: Vec<ResolvedCollaborationItem>,
    bytes: usize,
}

impl ResolvedTurnAdmission {
    /// Frozen admission identity, scoped to its collaboration.
    pub fn reference(&self) -> &CollaborationItemRef {
        &self.reference
    }
    /// Original boundary and selected references, unaffected by later amendments.
    pub fn admission(&self) -> &TurnAdmission {
        &self.admission
    }
    /// Exact attributed sources in canonical order.
    pub fn items(&self) -> &[ResolvedCollaborationItem] {
        &self.items
    }
    /// Semantic source bytes used by the separate materialization-cache limit.
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }
}

impl CollaborationLedger {
    /// Prepares a logical turn start; reusing its item identity preserves the frozen original.
    pub fn prepare_turn(
        &self,
        id: CollaborationItemId,
        boundary: TurnBoundary,
        previous: Option<CollaborationItemRef>,
    ) -> Result<Preparation, CollaborationError> {
        if let Some(record) = self.record_by_id(&id) {
            return match &record.event {
                CollaborationEvent::TurnAdmitted { admission }
                    if admission.boundary == boundary && admission.previous == previous =>
                {
                    Ok(Preparation::Existing(record.receipt()))
                }
                _ => Err(CollaborationError::ItemIdentityConflict),
            };
        }
        let items = self.eligible_items(&boundary.recipient, previous.as_ref())?;
        self.prepare(
            id,
            CollaborationEvent::TurnAdmitted {
                admission: TurnAdmission {
                    boundary,
                    previous,
                    items,
                },
            },
        )
    }

    pub(super) fn validate_turn_admission(
        &self,
        admission: &TurnAdmission,
    ) -> Result<(), CollaborationError> {
        let boundary = &admission.boundary;
        boundary.recipient.validate()?;
        validate_id(boundary.head.as_str())?;
        validate_id(boundary.turn.as_str())?;
        if let Some(parent) = &boundary.parent {
            validate_id(parent.as_str())?;
        }
        if !self.has_endpoint(&boundary.recipient) {
            return Err(CollaborationError::UnknownEndpoint);
        }
        if self.records().iter().any(|record| matches!(&record.event,
            CollaborationEvent::TurnAdmitted { admission: old }
            if old.boundary.recipient.conversation == boundary.recipient.conversation && old.boundary.turn == boundary.turn)) {
            return Err(CollaborationError::DuplicateTurnAdmission);
        }
        let expected = self.eligible_items(&boundary.recipient, admission.previous.as_ref())?;
        if expected.is_empty() {
            return Err(CollaborationError::NoPendingItems);
        }
        if expected != admission.items {
            return Err(CollaborationError::StaleTurnAdmission);
        }
        Ok(())
    }

    fn eligible_items(
        &self,
        recipient: &MailEndpoint,
        previous: Option<&CollaborationItemRef>,
    ) -> Result<Vec<CollaborationItemRef>, CollaborationError> {
        let after = if let Some(reference) = previous {
            let record = self.referenced_record(reference)?;
            let CollaborationEvent::TurnAdmitted { admission } = &record.event else {
                return Err(CollaborationError::InvalidReference);
            };
            if &admission.boundary.recipient != recipient {
                return Err(CollaborationError::ForeignReference);
            }
            record.sequence.0
        } else {
            0
        };
        let mut items = Vec::new();
        let mut bytes = 0;
        for record in self
            .records()
            .iter()
            .filter(|record| record.sequence.0 > after)
        {
            let relevant = match &record.event {
                CollaborationEvent::MailAccepted { mail } => &mail.to == recipient,
                CollaborationEvent::DelegationCreated {
                    delegator, worker, ..
                } => delegator == recipient || worker == recipient,
                CollaborationEvent::TaskAmended { delegation, .. } => self
                    .delegation(delegation)
                    .is_some_and(|task| &task.delegator == recipient || &task.worker == recipient),
                CollaborationEvent::ObjectionRaised { author, .. } => author == recipient,
                CollaborationEvent::TurnAdmitted { .. } => false,
            };
            if !relevant {
                continue;
            }
            bytes += source_bytes(record);
            if items.len() >= MAX_TURN_SOURCE_ITEMS || bytes > MAX_TURN_SOURCE_BYTES {
                return Err(CollaborationError::TurnSourceCapacity);
            }
            items.push(self.reference(record));
        }
        Ok(items)
    }

    /// Resolves the immutable prefix even if later mail or amendments have arrived.
    pub fn resolve_turn(
        &self,
        reference: &CollaborationItemRef,
    ) -> Result<Arc<ResolvedTurnAdmission>, CollaborationError> {
        let record = self.referenced_record(reference)?;
        let CollaborationEvent::TurnAdmitted { admission } = &record.event else {
            return Err(CollaborationError::InvalidReference);
        };
        let selected: BTreeSet<_> = admission.items.iter().map(|item| &item.item).collect();
        let mut revisions = BTreeMap::new();
        let mut items = Vec::new();
        let mut bytes = 0;
        for source in self
            .records()
            .iter()
            .take_while(|source| source.sequence < record.sequence)
        {
            let revision = match &source.event {
                CollaborationEvent::DelegationCreated { delegation, .. } => {
                    revisions.insert(delegation.clone(), DelegationRevision(0));
                    Some(DelegationRevision(0))
                }
                CollaborationEvent::TaskAmended { delegation, .. } => {
                    let value = revisions
                        .get_mut(delegation)
                        .expect("canonical amendment follows creation");
                    value.0 += 1;
                    Some(*value)
                }
                CollaborationEvent::ObjectionRaised { revision, .. } => Some(*revision),
                _ => None,
            };
            if selected.contains(&source.id) {
                bytes += source_bytes(source);
                items.push(ResolvedCollaborationItem {
                    reference: self.reference(source),
                    event: source.event.clone(),
                    task_revision: revision,
                });
            }
        }
        Ok(Arc::new(ResolvedTurnAdmission {
            reference: reference.clone(),
            admission: admission.clone(),
            items,
            bytes,
        }))
    }

    /// Resolves an accepted identity to its original canonical position.
    pub fn item_reference(
        &self,
        id: &CollaborationItemId,
    ) -> Result<CollaborationItemRef, CollaborationError> {
        self.record_by_id(id)
            .map(|record| self.reference(record))
            .ok_or(CollaborationError::InvalidReference)
    }

    fn reference(&self, record: &CollaborationRecord) -> CollaborationItemRef {
        CollaborationItemRef {
            collaboration: self.id().clone(),
            item: record.id.clone(),
            sequence: record.sequence,
        }
    }

    fn referenced_record(
        &self,
        reference: &CollaborationItemRef,
    ) -> Result<&CollaborationRecord, CollaborationError> {
        reference.validate()?;
        if &reference.collaboration != self.id() {
            return Err(CollaborationError::ForeignReference);
        }
        self.record_by_id(&reference.item)
            .filter(|record| record.sequence == reference.sequence)
            .ok_or(CollaborationError::InvalidReference)
    }
}

fn source_bytes(record: &CollaborationRecord) -> usize {
    record.id.as_str().len()
        + match &record.event {
            CollaborationEvent::MailAccepted { mail } => mail.retained_bytes(),
            CollaborationEvent::DelegationCreated {
                delegation,
                delegator,
                worker,
                task,
            } => {
                delegation.as_str().len() + delegator.bytes() + worker.bytes() + task.as_str().len()
            }
            CollaborationEvent::TaskAmended {
                delegation,
                task,
                author,
                ..
            } => {
                let author_bytes = match author {
                    super::DelegationAuthor::User => 0,
                    super::DelegationAuthor::Agent(endpoint) => endpoint.bytes(),
                };
                delegation.as_str().len() + task.as_str().len() + author_bytes
            }
            CollaborationEvent::ObjectionRaised {
                delegation,
                author,
                summary,
                ..
            } => delegation.as_str().len() + author.bytes() + summary.as_str().len(),
            CollaborationEvent::TurnAdmitted { .. } => 0,
        }
}
