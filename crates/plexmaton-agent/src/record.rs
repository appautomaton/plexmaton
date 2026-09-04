//! The live turn's boundary onto the canonical session journal.
//!
//! Completed semantic facts enter [`SessionJournal`] once. Provider requests rebuild from its
//! selected head; `Reaction::records` exposes those same mutations to the runtime. The remaining
//! counter numbers transient live events, including provider deltas that are deliberately not
//! durable session facts (JRN-5, JRN-6).

use plexmaton_core::{
    AgentId, AgentStatus, ApprovalId, AttentionId, EventSequence, HeadName, JournalRecordId,
    SessionEntryId, SessionEvent, SessionEventEnvelope, SessionId, ToolCallId, TranscriptItemId,
    TranscriptRole, TurnId,
};

use crate::interface::Reaction;
use crate::journal::{
    JournalEntryPayload, JournalProjection, JournalRecord, SessionEntry, SessionJournal,
};
use crate::model::{ModelRequest, RequestItem};

mod recovery;

/// One agent's canonical journal plus its transient live-event delivery cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Record {
    agent_id: AgentId,
    head: HeadName,
    journal: SessionJournal,
    announced: bool,
    next_event: u64,
}

impl Record {
    /// Starts one ephemeral in-memory session for the non-persistent composition path.
    pub(crate) fn new(agent_id: AgentId) -> Self {
        let session_id = SessionId::new(format!("{agent_id}-session"))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        Self::for_session(agent_id, session_id)
    }

    pub(crate) fn for_session(agent_id: AgentId, session_id: SessionId) -> Self {
        Self {
            agent_id,
            head: HeadName::new("main")
                .unwrap_or_else(|error| unreachable!("the main head is valid: {error}")),
            journal: SessionJournal::new(session_id),
            announced: false,
            next_event: 1,
        }
    }

    pub(crate) fn from_journal(
        agent_id: AgentId,
        journal: SessionJournal,
    ) -> Result<Self, crate::JournalProjectionError> {
        let head = HeadName::new("main")
            .unwrap_or_else(|error| unreachable!("the main head is valid: {error}"));
        let projection = journal.project(&head)?;
        let announced = projection.events().iter().any(|envelope| {
            matches!(
                &envelope.event,
                SessionEvent::AgentCreated { agent_id: created, .. } if created == &agent_id
            )
        });
        if !announced {
            return Err(crate::JournalProjectionError::MissingAgent(agent_id));
        }
        let next_event = projection.events().last().map_or(1, |event| {
            event
                .sequence
                .get()
                .checked_add(1)
                .unwrap_or_else(|| unreachable!("projected event sequence can be extended"))
        });
        Ok(Self {
            agent_id,
            head,
            journal,
            announced,
            next_event,
        })
    }

    pub(crate) fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    pub(crate) fn journal(&self) -> &SessionJournal {
        &self.journal
    }

    pub(crate) const fn is_announced(&self) -> bool {
        self.announced
    }

    /// The conversation rebuilt from the same journal path persistence receives (JRN-5).
    pub(crate) fn request(&self) -> ModelRequest {
        self.journal
            .project(&self.head)
            .unwrap_or_else(|error| unreachable!("live facts must remain projectable: {error:?}"))
            .request()
            .clone()
    }

    pub(crate) fn items(&self) -> Vec<RequestItem> {
        self.request().items
    }

    pub(crate) fn contains_tool_call(&self, call_id: &ToolCallId) -> bool {
        self.journal
            .path(&self.head)
            .unwrap_or_else(|error| unreachable!("the live head remains valid: {error:?}"))
            .iter()
            .any(|entry| {
                matches!(
                    &entry.payload,
                    JournalEntryPayload::ToolCallRequested { call, .. }
                        if &call.call_id == call_id
                )
            })
    }

    pub(crate) fn rebuild_projection(&mut self) -> JournalProjection {
        let projection = self
            .journal
            .project(&self.head)
            .unwrap_or_else(|error| unreachable!("live facts must remain projectable: {error:?}"));
        self.next_event = projection.events().last().map_or(1, |event| {
            event
                .sequence
                .get()
                .checked_add(1)
                .unwrap_or_else(|| unreachable!("projected event sequence cannot be extended"))
        });
        projection
    }

    /// Appends one complete fact to the in-memory journal and returns the exact mutation outward.
    pub(crate) fn commit(&mut self, payload: JournalEntryPayload, reaction: &mut Reaction) {
        let announces_agent = matches!(
            &payload,
            JournalEntryPayload::AgentCreated { agent_id, .. } if agent_id == &self.agent_id
        );
        let sequence = self.journal.next_sequence();
        let ordinal = sequence.get();
        let session_id = self.journal.session_id();
        let record_id = JournalRecordId::new(format!("{session_id}-record-{ordinal}"))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let entry_id = SessionEntryId::new(format!("{session_id}-entry-{ordinal}"))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let parent_id = self
            .journal
            .head_target(&self.head)
            .unwrap_or_else(|error| unreachable!("the live head remains valid: {error:?}"))
            .cloned();
        let expected_head_revision = self
            .journal
            .head_revision(&self.head)
            .unwrap_or_else(|error| unreachable!("the live head remains valid: {error:?}"));
        let record = JournalRecord::AppendEntry {
            sequence,
            record_id,
            head: self.head.clone(),
            expected_head_revision,
            entry: Box::new(SessionEntry {
                id: entry_id,
                parent_id,
                payload,
            }),
        };
        self.journal.apply(record.clone()).unwrap_or_else(|error| {
            unreachable!("a locally prepared mutation is valid: {error:?}")
        });
        self.announced |= announces_agent;
        reaction.records.push(record);
    }

    pub(crate) fn finish_turn(
        &mut self,
        turn_id: TurnId,
        outcome: crate::TurnOutcome,
        at: crate::TurnFinishedAt,
        reaction: &mut Reaction,
    ) {
        let sequence = self.journal.next_sequence();
        let session_id = self.journal.session_id();
        let record_id = JournalRecordId::new(format!("{session_id}-record-{}", sequence.get()))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let semantic_boundary = self
            .journal
            .head_target(&self.head)
            .unwrap_or_else(|error| unreachable!("the live head remains valid: {error:?}"))
            .cloned()
            .unwrap_or_else(|| unreachable!("a started turn has a semantic entry"));
        let expected_head_revision = self
            .journal
            .head_revision(&self.head)
            .unwrap_or_else(|error| unreachable!("the live head remains valid: {error:?}"));
        let record = JournalRecord::TurnFinished {
            sequence,
            record_id,
            head: self.head.clone(),
            expected_head_revision,
            fact: crate::TurnFinished {
                agent_id: self.agent_id.clone(),
                turn_id,
                semantic_boundary,
                outcome,
                at,
            },
        };
        self.journal.apply(record.clone()).unwrap_or_else(|error| {
            unreachable!("a locally prepared terminal fact is valid: {error:?}")
        });
        reaction.records.push(record);
        self.emit(
            reaction,
            SessionEvent::AgentStatusChanged {
                agent_id: self.agent_id.clone(),
                status: AgentStatus::Idle,
            },
        );
    }

    /// Numbers one transient or journal-derived event for the live projection.
    pub(crate) fn emit(&mut self, reaction: &mut Reaction, event: SessionEvent) {
        let sequence = EventSequence::new(self.next_event);
        self.next_event = self
            .next_event
            .checked_add(1)
            .unwrap_or_else(|| unreachable!("live event sequence exhausted"));
        reaction
            .events
            .push(SessionEventEnvelope { sequence, event });
    }

    /// Transcript identity paired with the journal record that will be appended next.
    pub(crate) fn next_item_id(&self) -> TranscriptItemId {
        let ordinal = self.journal.next_sequence().get();
        TranscriptItemId::new(format!("{}-item-j{ordinal}", self.agent_id))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"))
    }

    /// A tool call's model identity also fixes its one transcript position.
    pub(crate) fn tool_item_id(&self, call_id: &ToolCallId) -> TranscriptItemId {
        TranscriptItemId::new(format!("{}-tool-{call_id}", self.agent_id))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"))
    }

    /// A turn starts at the next canonical record and needs no independent durable counter.
    pub(crate) fn next_turn_id(&self) -> TurnId {
        let ordinal = self.journal.next_sequence().get();
        TurnId::new(format!("{}-turn-j{ordinal}", self.agent_id))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"))
    }

    /// A streamed item is stable before its completed message enters the journal.
    pub(crate) fn stream_item_id(
        turn_id: &TurnId,
        step: u16,
        role: TranscriptRole,
    ) -> TranscriptItemId {
        let role = match role {
            TranscriptRole::Assistant => "assistant",
            TranscriptRole::Reasoning => "reasoning",
            TranscriptRole::User | TranscriptRole::System => {
                unreachable!("only provider-streamed roles have stream identities")
            }
        };
        TranscriptItemId::new(format!("{turn_id}-step-{step}-{role}"))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"))
    }

    /// Approval and attention identities are stable consequences of the unique call identity.
    pub(crate) fn approval_ids(&self, call_id: &ToolCallId) -> (ApprovalId, AttentionId) {
        let approval = ApprovalId::new(format!("{}-approval-{call_id}", self.agent_id))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let attention = AttentionId::new(format!("{}-attention-{call_id}", self.agent_id))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        (approval, attention)
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, AgentStatus, SessionEvent};

    use super::Record;
    use crate::UnixMillis;
    use crate::interface::Reaction;
    use crate::journal::JournalEntryPayload;
    use crate::model::RequestItem;

    fn record() -> Record {
        Record::new(AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")))
    }

    #[test]
    fn one_record_numbers_one_live_stream_from_one() {
        let mut record = record();
        let mut reaction = Reaction::default();

        for _ in 0..3 {
            let item_id = record.next_item_id();
            record.emit(
                &mut reaction,
                SessionEvent::RuntimeWarning {
                    agent_id: record.agent_id().clone(),
                    item_id,
                    message: "noticed".to_owned(),
                },
            );
        }

        let sequences: Vec<_> = reaction
            .events
            .iter()
            .map(|envelope| envelope.sequence.get())
            .collect();
        assert_eq!(sequences, [1, 2, 3]);
    }

    #[test]
    fn every_transcript_identity_is_new_and_names_its_agent() {
        let mut record = record();
        let mut reaction = Reaction::default();
        record.commit(
            JournalEntryPayload::AgentCreated {
                agent_id: record.agent_id().clone(),
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
            &mut reaction,
        );
        let first = record.next_item_id();
        record.commit(
            JournalEntryPayload::RuntimeWarning {
                agent_id: record.agent_id().clone(),
                item_id: first.clone(),
                message: "first".to_owned(),
            },
            &mut reaction,
        );
        let second = record.next_item_id();

        assert_ne!(first, second);
        assert!(first.as_str().starts_with("agent-a"));
    }

    /// JRN-6: one completed fact feeds the journal, outward record and model projection once.
    #[test]
    fn jrn_6_one_commit_is_the_only_model_record() {
        let mut record = record();
        let mut reaction = Reaction::default();
        record.commit(
            JournalEntryPayload::AgentCreated {
                agent_id: record.agent_id().clone(),
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
            &mut reaction,
        );
        record.commit(
            JournalEntryPayload::TurnStarted {
                agent_id: record.agent_id().clone(),
                item_id: record.next_item_id(),
                turn_id: record.next_turn_id(),
                text: "hello".to_owned(),
                accepted_at: UnixMillis::EPOCH,
                opened_at: UnixMillis::EPOCH,
            },
            &mut reaction,
        );

        assert_eq!(reaction.records, record.journal().records());
        assert_eq!(
            record.items(),
            [RequestItem::User {
                text: "hello".to_owned(),
            }]
        );
    }
}
