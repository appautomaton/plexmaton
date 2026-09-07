use std::collections::BTreeMap;

use plexmaton_core::{
    AgentId, CollaborationId, CollaborationItemId, ConversationId, DelegationId, MailId,
};

use super::types::validate_id;
use super::{
    CollaborationError, CollaborationEvent, CollaborationLimits, CollaborationRecord,
    CollaborationSequence, DelegationAuthor, DelegationRevision, DelegationView, ItemReceipt,
    MailEndpoint, MailEnvelope,
};

/// Preparation either identifies an old admission or returns an exact record for the writer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Preparation {
    Existing(ItemReceipt),
    Append(CollaborationRecord),
}

/// Pure reduction of one bounded collaboration log; indexes are disposable projections (COL-1).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaborationLedger {
    id: CollaborationId,
    limits: CollaborationLimits,
    records: Vec<CollaborationRecord>,
    items: BTreeMap<CollaborationItemId, usize>,
    mails: BTreeMap<(MailEndpoint, MailId), usize>,
    delegations: BTreeMap<DelegationId, DelegationView>,
    endpoints: BTreeMap<ConversationId, AgentId>,
    workers: BTreeMap<ConversationId, DelegationId>,
    mail_bytes: usize,
}

impl CollaborationLedger {
    /// Starts an empty reduction under immutable validated bounds; performs no file creation.
    pub fn new(
        id: CollaborationId,
        limits: CollaborationLimits,
    ) -> Result<Self, CollaborationError> {
        validate_id(id.as_str())?;
        limits.validate()?;
        Ok(Self {
            id,
            limits,
            records: Vec::new(),
            items: BTreeMap::new(),
            mails: BTreeMap::new(),
            delegations: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            workers: BTreeMap::new(),
            mail_bytes: 0,
        })
    }

    /// Stable identity of the authority owning these records.
    #[must_use]
    pub const fn id(&self) -> &CollaborationId {
        &self.id
    }

    /// Header-level bounds that must also be used when replaying this log.
    #[must_use]
    pub const fn limits(&self) -> CollaborationLimits {
        self.limits
    }

    /// The canonical append order, including amendments and objections.
    #[must_use]
    pub fn records(&self) -> &[CollaborationRecord] {
        &self.records
    }

    /// Current task derived from creation and successful amendments, never from an objection.
    #[must_use]
    pub fn delegation(&self, id: &DelegationId) -> Option<&DelegationView> {
        self.delegations.get(id)
    }

    /// Accepted mail in canonical order; this does not claim model inclusion or consumption.
    pub fn mail_for<'a>(
        &'a self,
        endpoint: &'a MailEndpoint,
    ) -> impl Iterator<Item = (&'a CollaborationRecord, &'a MailEnvelope)> {
        self.records
            .iter()
            .filter_map(move |record| match &record.event {
                CollaborationEvent::MailAccepted { mail } if &mail.to == endpoint => {
                    Some((record, mail))
                }
                _ => None,
            })
    }

    /// Exact retries are resolved before capacity or revision checks (COL-1).
    pub fn prepare(
        &self,
        id: CollaborationItemId,
        event: CollaborationEvent,
    ) -> Result<Preparation, CollaborationError> {
        if let Some(index) = self.items.get(&id) {
            let existing = &self.records[*index];
            return if existing.event == event {
                Ok(Preparation::Existing(existing.receipt()))
            } else {
                Err(CollaborationError::ItemIdentityConflict)
            };
        }
        let record = CollaborationRecord {
            id,
            sequence: CollaborationSequence(self.records.len() as u64 + 1),
            event,
        };
        self.validate_record(&record)?;
        Ok(Preparation::Append(record))
    }

    /// Validates replay and new records through the same boundary, without changing state.
    pub fn validate_record(&self, record: &CollaborationRecord) -> Result<(), CollaborationError> {
        validate_id(record.id.as_str())?;
        if self.items.contains_key(&record.id) {
            return Err(CollaborationError::ItemIdentityConflict);
        }
        if record.sequence.0 != self.records.len() as u64 + 1 {
            return Err(CollaborationError::UnexpectedSequence);
        }
        if self.records.len() >= self.limits.items {
            return Err(CollaborationError::ItemCapacity);
        }
        match &record.event {
            CollaborationEvent::MailAccepted { mail } => self.validate_mail(mail),
            CollaborationEvent::DelegationCreated {
                delegation,
                delegator,
                worker,
                ..
            } => self.validate_creation(delegation, delegator, worker),
            CollaborationEvent::TaskAmended {
                delegation,
                expected,
                author,
                ..
            } => {
                let view = self
                    .delegations
                    .get(delegation)
                    .ok_or(CollaborationError::UnknownDelegation)?;
                match author {
                    DelegationAuthor::User => {
                        if *expected > view.revision
                            || view
                                .last_user_revision
                                .is_some_and(|revision| *expected < revision)
                        {
                            return Err(CollaborationError::StaleRevision);
                        }
                    }
                    DelegationAuthor::Agent(endpoint) => {
                        if endpoint != &view.delegator {
                            return Err(CollaborationError::WrongAuthor);
                        }
                        if *expected != view.revision {
                            return Err(CollaborationError::StaleRevision);
                        }
                        if view.last_user_revision.is_some() {
                            return Err(CollaborationError::UserAuthority);
                        }
                    }
                }
                Ok(())
            }
            CollaborationEvent::ObjectionRaised {
                delegation,
                revision,
                author,
                ..
            } => {
                let view = self
                    .delegations
                    .get(delegation)
                    .ok_or(CollaborationError::UnknownDelegation)?;
                if author != &view.delegator {
                    return Err(CollaborationError::WrongAuthor);
                }
                if *revision != view.revision {
                    return Err(CollaborationError::StaleRevision);
                }
                Ok(())
            }
        }
    }

    /// Reduces an acknowledged or replayed record. It performs no file, model or tool operation.
    pub fn apply(
        &mut self,
        record: CollaborationRecord,
    ) -> Result<ItemReceipt, CollaborationError> {
        self.validate_record(&record)?;
        match &record.event {
            CollaborationEvent::MailAccepted { mail } => {
                self.mail_bytes += mail.retained_bytes();
                self.mails
                    .insert((mail.from.clone(), mail.id.clone()), self.records.len());
            }
            CollaborationEvent::DelegationCreated {
                delegation,
                delegator,
                worker,
                task,
            } => {
                self.endpoints
                    .insert(delegator.conversation.clone(), delegator.agent.clone());
                self.endpoints
                    .insert(worker.conversation.clone(), worker.agent.clone());
                self.workers
                    .insert(worker.conversation.clone(), delegation.clone());
                self.delegations.insert(
                    delegation.clone(),
                    DelegationView {
                        delegator: delegator.clone(),
                        worker: worker.clone(),
                        task: task.clone(),
                        revision: DelegationRevision(0),
                        author: DelegationAuthor::Agent(delegator.clone()),
                        last_user_revision: None,
                    },
                );
            }
            CollaborationEvent::TaskAmended {
                delegation,
                author,
                task,
                ..
            } => {
                let view = self
                    .delegations
                    .get_mut(delegation)
                    .expect("validated delegation exists");
                view.revision.0 += 1; // COL-2 caps all records far below revision exhaustion.
                view.task = task.clone();
                view.author = author.clone();
                if *author == DelegationAuthor::User {
                    view.last_user_revision = Some(view.revision);
                }
            }
            CollaborationEvent::ObjectionRaised { .. } => {}
        }
        let receipt = record.receipt();
        self.items.insert(record.id.clone(), self.records.len());
        self.records.push(record);
        Ok(receipt)
    }

    fn validate_mail(&self, mail: &MailEnvelope) -> Result<(), CollaborationError> {
        mail.validate()?;
        for endpoint in [&mail.from, &mail.to] {
            if self.endpoints.get(&endpoint.conversation) != Some(&endpoint.agent) {
                return Err(CollaborationError::UnknownEndpoint);
            }
        }
        if self
            .mails
            .contains_key(&(mail.from.clone(), mail.id.clone()))
        {
            return Err(CollaborationError::MailIdentityConflict);
        }
        if self.records.len() >= self.limits.items - self.limits.control_items {
            return Err(CollaborationError::ItemCapacity);
        }
        if self.mail_bytes + mail.retained_bytes() > self.limits.mail_bytes {
            return Err(CollaborationError::MailCapacity);
        }
        Ok(())
    }

    fn validate_creation(
        &self,
        id: &DelegationId,
        delegator: &MailEndpoint,
        worker: &MailEndpoint,
    ) -> Result<(), CollaborationError> {
        validate_id(id.as_str())?;
        delegator.validate()?;
        worker.validate()?;
        if delegator.conversation == worker.conversation {
            return Err(CollaborationError::SameSession);
        }
        if self.delegations.contains_key(id) {
            return Err(CollaborationError::DuplicateDelegation);
        }
        if self.workers.contains_key(&worker.conversation) {
            return Err(CollaborationError::WorkerAlreadyAssigned);
        }
        if self.delegations.len() >= self.limits.delegations {
            return Err(CollaborationError::DelegationCapacity);
        }
        for endpoint in [delegator, worker] {
            if self
                .endpoints
                .get(&endpoint.conversation)
                .is_some_and(|agent| agent != &endpoint.agent)
            {
                return Err(CollaborationError::EndpointIdentityConflict);
            }
        }
        let mut ancestor = &delegator.conversation;
        loop {
            if ancestor == &worker.conversation {
                return Err(CollaborationError::DelegationCycle);
            }
            let Some(parent) = self
                .workers
                .get(ancestor)
                .and_then(|id| self.delegations.get(id))
            else {
                break;
            };
            ancestor = &parent.delegator.conversation;
        }
        Ok(())
    }
}
