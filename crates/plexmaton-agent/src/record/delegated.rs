//! Process-local placement of durable collaboration facts in one session projection.

use plexmaton_core::{
    ConversationEntryId, ConversationEvent, ConversationEventEnvelope, EventSequence, HeadName,
};

use super::Record;
use crate::interface::Reaction;
use crate::journal::{JournalEntryPayload, JournalProjection, JournalRecord};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DelegatedProjection {
    pub(super) event: ConversationEvent,
    pub(super) placement: DelegatedPlacement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DelegatedPlacement {
    Roster,
    Collaboration {
        reference: crate::collaboration::CollaborationItemRef,
        fallback: Vec<ConversationEntryId>,
    },
    Tail,
}

impl Record {
    pub(crate) fn project_with_delegated(
        &self,
        head: &HeadName,
    ) -> Result<JournalProjection, crate::JournalProjectionError> {
        self.journal
            .project(head)
            .map(|projection| self.with_delegated(projection))
    }

    pub(crate) fn project_at_with_delegated(
        &self,
        target: Option<&ConversationEntryId>,
    ) -> Result<JournalProjection, crate::JournalProjectionError> {
        self.journal
            .project_at(target)
            .map(|projection| self.with_delegated(projection))
    }

    pub(super) fn with_delegated(&self, mut projection: JournalProjection) -> JournalProjection {
        if self.delegated.is_empty() {
            return projection;
        }
        let base = projection.events().to_vec();
        let roster_offset = base
            .iter()
            .position(|envelope| {
                matches!(
                    &envelope.event,
                    ConversationEvent::AgentCreated { agent_id, .. } if agent_id == &self.agent_id
                )
            })
            .map_or(0, |offset| offset.saturating_add(1));
        let mut before = vec![Vec::new(); base.len().saturating_add(1)];
        let mut roster = Vec::new();
        let mut tail = Vec::new();
        for delegated in &self.delegated {
            match &delegated.placement {
                DelegatedPlacement::Roster => roster.push(delegated.event.clone()),
                DelegatedPlacement::Collaboration {
                    reference,
                    fallback,
                } => {
                    let direct = self
                        .journal
                        .records()
                        .iter()
                        .find_map(|record| match record {
                            JournalRecord::AppendEntry { entry, .. }
                                if matches!(
                                    &entry.payload,
                                    JournalEntryPayload::CollaborationItemLinked {
                                        agent_id,
                                        reference: linked,
                                    } if agent_id == &self.agent_id && linked == reference
                                ) && projection.event_offset(&entry.id).is_some() =>
                            {
                                Some(&entry.id)
                            }
                            _ => None,
                        });
                    if let Some(offset) = direct
                        .and_then(|entry| projection.event_offset(entry))
                        .or_else(|| {
                            fallback
                                .iter()
                                .find_map(|entry| projection.event_offset(entry))
                        })
                    {
                        before[offset].push(delegated.event.clone());
                    } else {
                        tail.push(delegated.event.clone());
                    }
                }
                DelegatedPlacement::Tail => tail.push(delegated.event.clone()),
            }
        }
        let mut events = Vec::with_capacity(base.len().saturating_add(self.delegated.len()));
        let mut placement_boundaries = Vec::with_capacity(base.len().saturating_add(1));
        for (offset, at) in before.iter_mut().enumerate() {
            placement_boundaries.push(events.len());
            if offset == roster_offset {
                events.append(&mut roster);
            }
            events.append(at);
            if let Some(envelope) = base.get(offset) {
                events.push(envelope.event.clone());
            }
        }
        events.append(&mut tail);
        projection.replace_events(
            events
                .into_iter()
                .enumerate()
                .map(|(offset, event)| ConversationEventEnvelope {
                    sequence: EventSequence::new(offset as u64 + 1),
                    event,
                })
                .collect(),
            &placement_boundaries,
        );
        projection
    }

    pub(crate) fn project_delegated(
        &mut self,
        event: ConversationEvent,
        placement: DelegatedPlacement,
        reaction: &mut Reaction,
    ) {
        self.delegated.push(DelegatedProjection {
            event: event.clone(),
            placement,
        });
        self.emit(reaction, event);
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, CollaborationId, CollaborationItemId, ConversationEvent, HeadName,
        JournalRecordId, TranscriptItemId,
    };

    use super::DelegatedPlacement;
    use crate::collaboration::{CollaborationItemRef, CollaborationSequence};
    use crate::interface::Reaction;
    use crate::journal::{JournalEntryPayload, JournalRecord};
    use crate::record::Record;

    /// ENT-1/JRN-5: selected branches rebuild the same external item once. A branch containing its
    /// sender link uses that position; an older branch keeps the item in the compatibility suffix.
    #[test]
    fn selected_branches_place_or_suffix_one_delegated_reference_without_duplication() {
        let mut record = Record::new(AgentId::new("agent-a").expect("agent"));
        let mut reaction = Reaction::default();
        record.commit(
            JournalEntryPayload::AgentCreated {
                agent_id: record.agent_id().clone(),
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
            &mut reaction,
        );
        for (item, message) in [("before", "before"), ("after", "after")] {
            if item == "after" {
                record.commit(
                    JournalEntryPayload::CollaborationItemLinked {
                        agent_id: record.agent_id().clone(),
                        reference: reference(),
                    },
                    &mut reaction,
                );
            }
            record.commit(
                JournalEntryPayload::RuntimeWarning {
                    agent_id: record.agent_id().clone(),
                    item_id: TranscriptItemId::new(item).expect("item"),
                    message: message.to_owned(),
                },
                &mut reaction,
            );
        }
        let main = record.selected_head().clone();
        let before = record.journal.path(&main).expect("main path")[1].id.clone();
        let legacy = HeadName::new("legacy").expect("head");
        let sequence = record.journal.next_sequence();
        record
            .journal
            .apply(JournalRecord::CreateHead {
                sequence,
                record_id: JournalRecordId::new(format!("record-{}", sequence.get()))
                    .expect("record"),
                head: legacy.clone(),
                at: Some(before),
            })
            .expect("create legacy branch");
        record.project_delegated(
            ConversationEvent::RuntimeWarning {
                agent_id: record.agent_id().clone(),
                item_id: TranscriptItemId::new("shared").expect("item"),
                message: "shared".to_owned(),
            },
            DelegatedPlacement::Collaboration {
                reference: reference(),
                fallback: Vec::new(),
            },
            &mut reaction,
        );

        let messages = |head: &HeadName| {
            record
                .project_with_delegated(head)
                .expect("branch projection")
                .events()
                .iter()
                .filter_map(|envelope| match &envelope.event {
                    ConversationEvent::RuntimeWarning { message, .. } => Some(message.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(messages(&main), ["before", "shared", "after"]);
        assert_eq!(messages(&legacy), ["before", "shared"]);
    }

    /// ENT-1: a later compatibility anchor cannot supersede the first durable session link.
    #[test]
    fn a_direct_link_wins_over_a_later_fallback_anchor() {
        let mut record = Record::new(AgentId::new("agent-a").expect("agent"));
        let mut reaction = Reaction::default();
        record.commit(
            JournalEntryPayload::AgentCreated {
                agent_id: record.agent_id().clone(),
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
            &mut reaction,
        );
        warning(&mut record, &mut reaction, "before");
        record.commit(
            JournalEntryPayload::CollaborationItemLinked {
                agent_id: record.agent_id().clone(),
                reference: reference(),
            },
            &mut reaction,
        );
        warning(&mut record, &mut reaction, "after");
        warning(&mut record, &mut reaction, "recipient-boundary");
        let link = record
            .journal
            .path(record.selected_head())
            .expect("selected path")
            .iter()
            .find(|entry| {
                matches!(
                    &entry.payload,
                    JournalEntryPayload::CollaborationItemLinked { .. }
                )
            })
            .expect("link entry")
            .id
            .clone();
        let boundary = record
            .journal
            .path(record.selected_head())
            .expect("selected path")
            .last()
            .expect("boundary entry")
            .id
            .clone();
        record.project_delegated(
            ConversationEvent::RuntimeWarning {
                agent_id: record.agent_id().clone(),
                item_id: TranscriptItemId::new("shared").expect("item"),
                message: "shared".to_owned(),
            },
            DelegatedPlacement::Collaboration {
                reference: reference(),
                fallback: vec![boundary],
            },
            &mut reaction,
        );
        let projection = record
            .project_with_delegated(record.selected_head())
            .expect("reopened projection");
        let linked_offset = projection.event_offset(&link).expect("link placement");
        assert!(matches!(
            &projection.events()[linked_offset].event,
            ConversationEvent::RuntimeWarning { message, .. } if message == "shared"
        ));
        let reopened: Vec<_> = projection
            .events()
            .iter()
            .filter_map(|envelope| match &envelope.event {
                ConversationEvent::RuntimeWarning { message, .. } => Some(message.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            reopened,
            ["before", "shared", "after", "recipient-boundary"]
        );
    }

    fn warning(record: &mut Record, reaction: &mut Reaction, message: &str) {
        let event = ConversationEvent::RuntimeWarning {
            agent_id: record.agent_id().clone(),
            item_id: TranscriptItemId::new(message).expect("item"),
            message: message.to_owned(),
        };
        record.commit(
            JournalEntryPayload::RuntimeWarning {
                agent_id: record.agent_id().clone(),
                item_id: TranscriptItemId::new(message).expect("item"),
                message: message.to_owned(),
            },
            reaction,
        );
        record.emit(reaction, event);
    }

    fn reference() -> CollaborationItemRef {
        CollaborationItemRef {
            collaboration: CollaborationId::new("collaboration").expect("collaboration"),
            item: CollaborationItemId::new("shared-fact").expect("item"),
            sequence: CollaborationSequence(1),
        }
    }
}
