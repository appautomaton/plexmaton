//! The live turn's boundary onto the canonical session journal.
//!
//! Completed semantic facts enter [`ConversationJournal`] once. Provider requests rebuild from its
//! selected head; `Reaction::records` exposes those same mutations to the runtime. The remaining
//! counter numbers transient live events, including provider deltas that are deliberately not
//! durable session facts (JRN-5, JRN-6).

use plexmaton_core::{
    AgentId, AgentStatus, ApprovalId, AttentionId, ConversationEntryId, ConversationEvent,
    ConversationEventEnvelope, ConversationId, EventSequence, HeadName, JournalRecordId,
    ToolCallId, TranscriptItemId, TranscriptRole, TurnId,
};

use crate::interface::Reaction;
use crate::journal::{
    ConversationEntry, ConversationJournal, JournalEntryPayload, JournalProjection, JournalRecord,
};
use crate::model::{ContextAtom, ModelOutputPosition, ModelRequest};
use crate::{
    CompactionAttemptFinished, CompactionCheckpoint, CompactionPlan, CompactionSource,
    ConversationMetadata, RequestAttemptAuthorized, RequestAttemptId, RequestAttemptOwner,
    RequestAttemptTerminal, RequestEnvironment, UnixMillis,
};

mod navigation;
mod recovery;
mod retry;
mod tree_edit;

/// One agent's canonical journal plus its transient live-event delivery cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Record {
    agent_id: AgentId,
    journal: ConversationJournal,
    announced: bool,
    next_event: u64,
}

pub(crate) enum RequestAttemptCommitError {
    Journal(crate::JournalError),
    Projection(crate::JournalProjectionError),
}

impl Record {
    /// Starts one ephemeral in-memory session for the non-persistent composition path.
    pub(crate) fn new(agent_id: AgentId) -> Self {
        let session_id = ConversationId::new(format!("{agent_id}-session"))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        Self::for_conversation(
            agent_id,
            ConversationMetadata::new(session_id, UnixMillis::EPOCH),
        )
    }

    pub(crate) fn for_conversation(agent_id: AgentId, metadata: ConversationMetadata) -> Self {
        Self {
            agent_id,
            journal: ConversationJournal::with_metadata(metadata),
            announced: false,
            next_event: 1,
        }
    }

    pub(crate) fn from_journal(
        agent_id: AgentId,
        journal: ConversationJournal,
    ) -> Result<Self, crate::JournalProjectionError> {
        let projection = journal.project(journal.selected_head())?;
        let announced = projection.events().iter().any(|envelope| {
            matches!(
                &envelope.event,
                ConversationEvent::AgentCreated { agent_id: created, .. } if created == &agent_id
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
            journal,
            announced,
            next_event,
        })
    }

    pub(crate) fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    pub(crate) fn journal(&self) -> &ConversationJournal {
        &self.journal
    }

    pub(crate) fn selected_head(&self) -> &HeadName {
        self.journal.selected_head()
    }

    pub(crate) const fn is_announced(&self) -> bool {
        self.announced
    }

    /// The conversation rebuilt from the same journal path persistence receives (JRN-5).
    pub(crate) fn request(&self) -> ModelRequest {
        self.journal
            .project(self.selected_head())
            .unwrap_or_else(|error| unreachable!("live facts must remain projectable: {error:?}"))
            .into_request()
    }

    pub(crate) fn atoms(&self) -> Vec<ContextAtom> {
        self.request().atoms
    }

    pub(crate) fn contains_tool_call(&self, call_id: &ToolCallId) -> bool {
        self.journal
            .path(self.selected_head())
            .unwrap_or_else(|error| unreachable!("the live head remains valid: {error:?}"))
            .iter()
            .any(|entry| {
                matches!(
                    &entry.payload,
                    JournalEntryPayload::AssistantOutput { output, .. }
                        if output.tool_calls().any(|call| &call.call_id == call_id)
                )
            })
    }

    pub(crate) fn rebuild_projection(&mut self) -> JournalProjection {
        let projection = self
            .journal
            .project(self.selected_head())
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
        let session_id = self.journal.conversation_id();
        let record_id = JournalRecordId::new(format!("{session_id}-record-{ordinal}"))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let entry_id = ConversationEntryId::new(format!("{session_id}-entry-{ordinal}"))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let parent_id = self
            .journal
            .head_target(self.selected_head())
            .unwrap_or_else(|error| unreachable!("the live head remains valid: {error:?}"))
            .cloned();
        let expected_head_revision = self
            .journal
            .head_revision(self.selected_head())
            .unwrap_or_else(|error| unreachable!("the live head remains valid: {error:?}"));
        let record = JournalRecord::AppendEntry {
            sequence,
            record_id,
            head: self.selected_head().clone(),
            expected_head_revision,
            entry: Box::new(ConversationEntry {
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
        let session_id = self.journal.conversation_id();
        let record_id = JournalRecordId::new(format!("{session_id}-record-{}", sequence.get()))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let semantic_boundary = self
            .journal
            .head_target(self.selected_head())
            .unwrap_or_else(|error| unreachable!("the live head remains valid: {error:?}"))
            .cloned()
            .unwrap_or_else(|| unreachable!("a started turn has a semantic entry"));
        let expected_head_revision = self
            .journal
            .head_revision(self.selected_head())
            .unwrap_or_else(|error| unreachable!("the live head remains valid: {error:?}"));
        let record = JournalRecord::TurnFinished {
            sequence,
            record_id,
            head: self.selected_head().clone(),
            expected_head_revision,
            fact: crate::TurnFinished {
                agent_id: self.agent_id.clone(),
                turn_id: turn_id.clone(),
                semantic_boundary,
                outcome,
                at,
            },
        };
        self.journal.apply(record.clone()).unwrap_or_else(|error| {
            unreachable!("a locally prepared terminal fact is valid: {error:?}")
        });
        reaction.records.push(record);
        if let Some(event) = self
            .journal
            .unfinished_turn_usage_event(self.selected_head(), &turn_id)
            .unwrap_or_else(|error| unreachable!("accepted attempts have valid totals: {error:?}"))
        {
            self.emit(reaction, event);
        }
        self.emit(
            reaction,
            ConversationEvent::AgentStatusChanged {
                agent_id: self.agent_id.clone(),
                status: AgentStatus::Idle,
            },
        );
    }

    pub(crate) fn next_request_attempt_id(&self) -> RequestAttemptId {
        RequestAttemptId::new(format!(
            "request-attempt-j{}",
            self.journal.next_sequence().get()
        ))
        .unwrap_or_else(|error| unreachable!("a bounded formatted identity is valid: {error}"))
    }

    pub(crate) fn compaction_source(&self) -> Result<CompactionSource, crate::JournalError> {
        self.journal.compaction_source(self.selected_head())
    }

    pub(crate) fn authorize_compaction_attempt(
        &mut self,
        attempt_id: RequestAttemptId,
        plan: &CompactionPlan,
        authorized_at: UnixMillis,
        reaction: &mut Reaction,
    ) -> Result<(), crate::JournalError> {
        if &self.compaction_source()? != plan.source() {
            return Err(crate::JournalError::CompactionSourceChanged);
        }
        let sequence = self.journal.next_sequence();
        let record_id = JournalRecordId::new(format!(
            "{}-record-{}",
            self.journal.conversation_id(),
            sequence.get()
        ))
        .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let record = JournalRecord::RequestAttemptAuthorized {
            sequence,
            record_id,
            head: self.selected_head().clone(),
            expected_head_revision: plan.source().head_revision(),
            fact: RequestAttemptAuthorized::new(
                attempt_id,
                RequestAttemptOwner::Compaction {
                    compaction_id: plan.id().clone(),
                },
                plan.source().boundary().clone(),
                plan.environment().clone(),
                authorized_at,
            ),
        };
        self.journal.apply(record.clone())?;
        reaction.records.push(record);
        Ok(())
    }

    pub(crate) fn authorize_request_attempt(
        &mut self,
        attempt_id: RequestAttemptId,
        owner: RequestAttemptOwner,
        environment: RequestEnvironment,
        authorized_at: UnixMillis,
        reaction: &mut Reaction,
    ) -> Result<(), crate::JournalError> {
        let sequence = self.journal.next_sequence();
        let record_id = JournalRecordId::new(format!(
            "{}-record-{}",
            self.journal.conversation_id(),
            sequence.get()
        ))
        .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let semantic_boundary = self
            .journal
            .head_target(self.selected_head())?
            .cloned()
            .ok_or_else(|| crate::JournalError::MissingTurn(owner_turn_id(&owner)))?;
        let expected_head_revision = self.journal.head_revision(self.selected_head())?;
        let record = JournalRecord::RequestAttemptAuthorized {
            sequence,
            record_id,
            head: self.selected_head().clone(),
            expected_head_revision,
            fact: RequestAttemptAuthorized::new(
                attempt_id,
                owner,
                semantic_boundary,
                environment,
                authorized_at,
            ),
        };
        self.journal.apply(record.clone())?;
        reaction.records.push(record);
        Ok(())
    }

    pub(crate) fn finish_request_attempt(
        &mut self,
        terminal: &RequestAttemptTerminal,
        reaction: &mut Reaction,
    ) -> Result<(), RequestAttemptCommitError> {
        let sequence = self.journal.next_sequence();
        let record_id = JournalRecordId::new(format!(
            "{}-record-{}",
            self.journal.conversation_id(),
            sequence.get()
        ))
        .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let record = JournalRecord::RequestAttemptFinished {
            sequence,
            record_id,
            fact: terminal.clone(),
        };
        self.journal
            .validate_record(&record)
            .map_err(RequestAttemptCommitError::Journal)?;
        let usage_event = self
            .journal
            .preview_cumulative_usage_event(self.selected_head(), terminal)
            .map_err(RequestAttemptCommitError::Projection)?;
        self.journal
            .apply(record.clone())
            .map_err(RequestAttemptCommitError::Journal)?;
        reaction.records.push(record);
        if let Some(event) = usage_event {
            self.emit(reaction, event);
        }
        Ok(())
    }

    pub(crate) fn finish_compaction_attempt(
        &mut self,
        fact: CompactionAttemptFinished,
        reaction: &mut Reaction,
    ) -> Result<(), crate::JournalError> {
        let sequence = self.journal.next_sequence();
        let record_id = JournalRecordId::new(format!(
            "{}-record-{}",
            self.journal.conversation_id(),
            sequence.get()
        ))
        .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let record = JournalRecord::CompactionAttemptFinished {
            sequence,
            record_id,
            fact,
        };
        self.journal.apply(record.clone())?;
        reaction.records.push(record);
        Ok(())
    }

    pub(crate) fn commit_compaction_checkpoint(
        &mut self,
        plan: CompactionPlan,
        successful_attempt_id: RequestAttemptId,
        reaction: &mut Reaction,
    ) -> Result<(), crate::JournalError> {
        let sequence = self.journal.next_sequence();
        let ordinal = sequence.get();
        let session_id = self.journal.conversation_id();
        let record_id = JournalRecordId::new(format!("{session_id}-record-{ordinal}"))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let entry_id = ConversationEntryId::new(format!("{session_id}-entry-{ordinal}"))
            .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let record = JournalRecord::AppendEntry {
            sequence,
            record_id,
            head: self.selected_head().clone(),
            expected_head_revision: plan.source().head_revision(),
            entry: Box::new(ConversationEntry {
                id: entry_id,
                parent_id: Some(plan.source().boundary().clone()),
                payload: JournalEntryPayload::CompactionCheckpoint {
                    agent_id: self.agent_id.clone(),
                    checkpoint: Box::new(CompactionCheckpoint::new(plan, successful_attempt_id)),
                },
            }),
        };
        self.journal.apply(record.clone())?;
        reaction.records.push(record);
        Ok(())
    }

    /// Numbers one transient or journal-derived event for the live projection.
    pub(crate) fn emit(&mut self, reaction: &mut Reaction, event: ConversationEvent) {
        let sequence = EventSequence::new(self.next_event);
        self.next_event = self
            .next_event
            .checked_add(1)
            .unwrap_or_else(|| unreachable!("live event sequence exhausted"));
        reaction
            .events
            .push(ConversationEventEnvelope { sequence, event });
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
        position: ModelOutputPosition,
    ) -> TranscriptItemId {
        let role = match role {
            TranscriptRole::Assistant => "assistant",
            TranscriptRole::Reasoning => "reasoning",
            TranscriptRole::User | TranscriptRole::System => {
                unreachable!("only provider-streamed roles have stream identities")
            }
        };
        TranscriptItemId::new(format!(
            "{turn_id}-step-{step}-{role}-{}-{}",
            position.item(),
            position.part()
        ))
        .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"))
    }

    /// Approval and attention identities are stable consequences of the unique call identity.
    pub(crate) fn approval_ids(
        &self,
        turn_id: &TurnId,
        call_id: &ToolCallId,
    ) -> (ApprovalId, AttentionId) {
        let approval = ApprovalId::new(format!(
            "{}-{}-{turn_id}-{}-{}-approval-{call_id}",
            self.journal.conversation_id(),
            self.selected_head(),
            self.journal.next_sequence().get(),
            self.agent_id
        ))
        .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        let attention = AttentionId::new(format!(
            "{}-{}-{turn_id}-{}-{}-attention-{call_id}",
            self.journal.conversation_id(),
            self.selected_head(),
            self.journal.next_sequence().get(),
            self.agent_id
        ))
        .unwrap_or_else(|error| unreachable!("a formatted identity is valid: {error}"));
        (approval, attention)
    }
}

fn owner_turn_id(owner: &RequestAttemptOwner) -> TurnId {
    match owner {
        RequestAttemptOwner::AgentStep { step_id } => step_id.turn_id().clone(),
        RequestAttemptOwner::Compaction { .. } => {
            unreachable!("agent record authorizes only agent-step attempts")
        }
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{AgentId, AgentStatus, ConversationEvent};

    use super::Record;
    use crate::ContextAtomValue;
    use crate::UnixMillis;
    use crate::interface::Reaction;
    use crate::journal::JournalEntryPayload;

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
                ConversationEvent::RuntimeWarning {
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
        assert!(matches!(
            record.atoms().as_slice(),
            [atom] if atom.value() == &ContextAtomValue::User { text: "hello".to_owned() }
        ));
    }

    /// TRE-3: reopen uses the durable selected head, not a hardcoded `main`.
    #[test]
    fn tre_3_from_journal_projects_the_durable_selected_head() {
        use crate::journal::{HeadRevision, JournalRecord};
        use plexmaton_core::{HeadName, JournalRecordId};

        let mut live = record();
        let mut reaction = Reaction::default();
        live.commit(
            JournalEntryPayload::AgentCreated {
                agent_id: live.agent_id().clone(),
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
            &mut reaction,
        );
        let at = live
            .journal()
            .head_target(live.selected_head())
            .unwrap_or_else(|error| panic!("selected target: {error:?}"))
            .cloned();
        let mut journal = live.journal().clone();
        let destination =
            HeadName::new("rewound").unwrap_or_else(|error| panic!("destination: {error}"));
        journal
            .apply(JournalRecord::ForkAndSelectHead {
                sequence: journal.next_sequence(),
                record_id: JournalRecordId::new("record-fork")
                    .unwrap_or_else(|error| panic!("record id: {error}")),
                source: live.selected_head().clone(),
                expected_source_revision: journal
                    .head_revision(live.selected_head())
                    .unwrap_or_else(|error| panic!("source revision: {error:?}")),
                destination: destination.clone(),
                at,
            })
            .unwrap_or_else(|error| panic!("fork and select: {error:?}"));

        let restored = Record::from_journal(live.agent_id().clone(), journal)
            .unwrap_or_else(|error| panic!("from_journal: {error:?}"));
        assert_eq!(restored.selected_head(), &destination);
        assert_eq!(
            restored.journal().head_revision(&destination),
            Ok(HeadRevision::new(0))
        );
    }
}
