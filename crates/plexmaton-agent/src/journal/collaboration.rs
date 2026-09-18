use plexmaton_core::{AgentId, ConversationEntryId, HeadName, TurnId};

use super::{ConversationJournal, JournalEntryPayload, JournalError, JournalRecord};
use crate::collaboration::{
    CollaborationError, CollaborationItemRef, MailEndpoint, ResolvedTurnAdmission, TurnBoundary,
};

/// Exact selected-branch session fact proving that one collaboration admission entered a turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaborationInclusionOrigin {
    entry: ConversationEntryId,
    reference: CollaborationItemRef,
}

/// Exact selected-branch session anchor for one canonical collaboration fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaborationLinkOrigin {
    entry: ConversationEntryId,
    agent: AgentId,
    reference: CollaborationItemRef,
}

impl CollaborationLinkOrigin {
    #[must_use]
    pub const fn entry(&self) -> &ConversationEntryId {
        &self.entry
    }

    #[must_use]
    pub const fn agent(&self) -> &AgentId {
        &self.agent
    }

    #[must_use]
    pub const fn reference(&self) -> &CollaborationItemRef {
        &self.reference
    }
}

impl CollaborationInclusionOrigin {
    #[must_use]
    pub const fn entry(&self) -> &ConversationEntryId {
        &self.entry
    }

    #[must_use]
    pub const fn reference(&self) -> &CollaborationItemRef {
        &self.reference
    }
}

impl ConversationJournal {
    /// Session-side collaboration references on one selected branch, in session order.
    pub fn collaboration_links(
        &self,
        head: &HeadName,
    ) -> Result<Vec<CollaborationLinkOrigin>, JournalError> {
        Ok(self
            .path(head)?
            .into_iter()
            .filter_map(|entry| match &entry.payload {
                JournalEntryPayload::CollaborationItemLinked {
                    agent_id,
                    reference,
                } => Some(CollaborationLinkOrigin {
                    entry: entry.id.clone(),
                    agent: agent_id.clone(),
                    reference: reference.clone(),
                }),
                _ => None,
            })
            .collect())
    }

    pub(super) fn validate_collaboration_link(
        &self,
        agent: &AgentId,
        reference: &CollaborationItemRef,
        parent: Option<&ConversationEntryId>,
    ) -> Result<(), JournalError> {
        reference.validate().map_err(JournalError::Collaboration)?;
        let mut cursor = parent;
        let mut announced = false;
        while let Some(id) = cursor {
            let entry = self
                .entries
                .get(id)
                .ok_or_else(|| JournalError::MissingEntry(id.clone()))?;
            if matches!(
                &entry.payload,
                JournalEntryPayload::AgentCreated { agent_id, .. } if agent_id == agent
            ) {
                announced = true;
            }
            if matches!(
                &entry.payload,
                JournalEntryPayload::CollaborationItemLinked { reference: existing, .. }
                    if existing == reference
            ) {
                return Err(JournalError::Collaboration(
                    CollaborationError::InvalidReference,
                ));
            }
            cursor = entry.parent_id.as_ref();
        }
        if announced {
            Ok(())
        } else {
            Err(JournalError::Collaboration(
                CollaborationError::ForeignReference,
            ))
        }
    }

    /// Every collaboration inclusion on one branch, in provider order.
    pub fn collaboration_inclusions(
        &self,
        head: &HeadName,
    ) -> Result<Vec<CollaborationInclusionOrigin>, JournalError> {
        Ok(self
            .path(head)?
            .into_iter()
            .filter_map(|entry| match &entry.payload {
                JournalEntryPayload::CollaborationTurnStarted { reference, .. } => {
                    Some(CollaborationInclusionOrigin {
                        entry: entry.id.clone(),
                        reference: reference.clone(),
                    })
                }
                _ => None,
            })
            .collect())
    }

    /// Inclusion cursor is selected ancestry, not the latest global admission (CIN-2).
    pub fn previous_collaboration_inclusion(
        &self,
        head: &HeadName,
    ) -> Result<Option<CollaborationItemRef>, JournalError> {
        Ok(self
            .path(head)?
            .iter()
            .rev()
            .find_map(|entry| match &entry.payload {
                JournalEntryPayload::CollaborationTurnStarted { reference, .. } => {
                    Some(reference.clone())
                }
                _ => None,
            }))
    }

    /// Captures a proposed idle boundary; the runtime separately verifies that no commit is pending.
    pub fn collaboration_boundary(
        &self,
        head: &HeadName,
        agent: &AgentId,
        turn: TurnId,
    ) -> Result<TurnBoundary, JournalError> {
        if self.turn_starts.contains_key(&turn) {
            return Err(JournalError::DuplicateTurn(turn));
        }
        let state = self.head(head)?;
        if let Some(turn) = &state.open_turn {
            return Err(JournalError::UnstableTurnTarget(turn.clone()));
        }
        Ok(TurnBoundary {
            recipient: MailEndpoint {
                conversation: self.conversation_id().clone(),
                agent: agent.clone(),
            },
            head: head.clone(),
            head_revision: state.revision,
            parent: state.target.clone(),
            turn,
        })
    }

    pub(crate) fn validate_collaboration_start(
        &self,
        reference: &CollaborationItemRef,
        turn: &TurnId,
        open: Option<&TurnId>,
    ) -> Result<(), JournalError> {
        reference.validate().map_err(JournalError::Collaboration)?;
        self.validate_new_turn(turn, open)?;
        if self.entries.values().any(|entry| matches!(&entry.payload,
            JournalEntryPayload::CollaborationTurnStarted { reference: previous, .. } if previous == reference)) {
            return Err(JournalError::Collaboration(CollaborationError::InvalidReference));
        }
        Ok(())
    }

    /// Validates a materialized source against the exact persisted session mutation, including
    /// its historical head name/revision; later head renames do not rewrite that provenance.
    pub fn validate_collaboration_source(
        &self,
        source: &ConversationEntryId,
        resolved: &ResolvedTurnAdmission,
    ) -> Result<(), CollaborationError> {
        let admission = resolved.admission();
        let boundary = &admission.boundary;
        if &boundary.recipient.conversation != self.conversation_id() {
            return Err(CollaborationError::ForeignReference);
        }
        let sequence = self
            .entry_sequences
            .get(source)
            .ok_or(CollaborationError::InvalidReference)?;
        let index = usize::try_from(sequence.get() - 1)
            .map_err(|_| CollaborationError::InvalidReference)?;
        let record = self
            .records
            .get(index)
            .ok_or(CollaborationError::InvalidReference)?;
        let JournalRecord::AppendEntry {
            head,
            expected_head_revision: revision,
            entry,
            ..
        } = record
        else {
            return Err(CollaborationError::InvalidReference);
        };
        let JournalEntryPayload::CollaborationTurnStarted {
            agent_id: agent,
            turn_id: turn,
            reference,
            ..
        } = &entry.payload
        else {
            return Err(CollaborationError::InvalidReference);
        };
        if reference != resolved.reference() {
            return Err(CollaborationError::InvalidReference);
        }
        if head != &boundary.head
            || revision != &boundary.head_revision
            || entry.parent_id != boundary.parent
            || agent != &boundary.recipient.agent
            || turn != &boundary.turn
        {
            return Err(CollaborationError::InvalidTurnBoundary);
        }
        let mut ancestor = entry.parent_id.as_ref();
        let mut previous = None;
        while let Some(id) = ancestor {
            let entry = self
                .entries
                .get(id)
                .ok_or(CollaborationError::InvalidTurnBoundary)?;
            if let JournalEntryPayload::CollaborationTurnStarted { reference, .. } = &entry.payload
            {
                previous = Some(reference);
                break;
            }
            ancestor = entry.parent_id.as_ref();
        }
        if previous != admission.previous.as_ref() {
            return Err(CollaborationError::InvalidTurnBoundary);
        }
        Ok(())
    }
}
