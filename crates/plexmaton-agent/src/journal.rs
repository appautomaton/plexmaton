//! The canonical session record's structural core.
//!
//! This module owns no file. A later adapter appends one prepared [`JournalRecord`] and only then
//! gives that same value to [`SessionJournal::apply`]. Keeping preparation and reduction pure makes
//! a JSONL reload the same operation as a live append (JRN-1, JRN-2).

use std::collections::{BTreeMap, BTreeSet};

use plexmaton_core::{AgentId, HeadName, JournalRecordId, SessionEntryId, SessionId, TurnId};

use crate::{
    ModelStepId, RequestAttempt, RequestAttemptId, RequestAttemptOwner, TurnFinished, UnixMillis,
};

mod accounting;
#[cfg(test)]
mod accounting_tests;
#[cfg(test)]
mod attempt_tests;
mod attempts;
mod error;
mod payload;
#[cfg(test)]
mod payload_tests;
mod projection;
mod record;
mod turns;
#[cfg(test)]
mod validation_tests;

pub use accounting::{RequestAccounting, RequestAccountingError};
pub use error::JournalError;
pub use payload::JournalEntryPayload;
pub(crate) use payload::PROCESS_RECOVERY_MESSAGE;
pub use projection::{JournalProjection, JournalProjectionError, RecoveryProjection};
pub use record::{HeadRevision, JournalRecord, JournalSequence, SessionEntry};

/// Immutable identity and chronology shared by every projection of one session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionMetadata {
    session_id: SessionId,
    created_at_unix_ms: UnixMillis,
}

impl SessionMetadata {
    /// Binds one identity to the externally observed instant it first existed.
    #[must_use]
    pub const fn new(session_id: SessionId, created_at_unix_ms: UnixMillis) -> Self {
        Self {
            session_id,
            created_at_unix_ms,
        }
    }

    /// Stable session identity.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Milliseconds since the Unix epoch when this session was created.
    #[must_use]
    pub const fn created_at_unix_ms(&self) -> UnixMillis {
        self.created_at_unix_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HeadState {
    target: Option<SessionEntryId>,
    revision: HeadRevision,
    open_turn: Option<TurnId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TurnStartState {
    agent_id: AgentId,
    entry_id: SessionEntryId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TurnFinishState {
    sequence: JournalSequence,
    fact: TurnFinished,
}

/// Deterministic in-memory reduction of one session's ordered records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionJournal {
    metadata: SessionMetadata,
    next_sequence: JournalSequence,
    records: Vec<JournalRecord>,
    record_ids: BTreeSet<JournalRecordId>,
    entries: BTreeMap<SessionEntryId, SessionEntry>,
    entry_sequences: BTreeMap<SessionEntryId, JournalSequence>,
    stable_entries: BTreeSet<SessionEntryId>,
    unstable_entry_turns: BTreeMap<SessionEntryId, TurnId>,
    heads: BTreeMap<HeadName, HeadState>,
    retired_heads: BTreeSet<HeadName>,
    turn_starts: BTreeMap<TurnId, TurnStartState>,
    turn_finishes: BTreeMap<TurnId, TurnFinishState>,
    model_steps: BTreeSet<ModelStepId>,
    last_model_step_indexes: BTreeMap<TurnId, u16>,
    request_attempts: BTreeMap<RequestAttemptId, RequestAttempt>,
    request_attempt_order: Vec<RequestAttemptId>,
    active_request_owners: BTreeMap<RequestAttemptOwner, RequestAttemptId>,
}

impl SessionJournal {
    /// Starts a deterministic synthetic session at the Unix epoch.
    #[must_use]
    pub fn new(session_id: SessionId) -> Self {
        Self::with_metadata(SessionMetadata::new(session_id, UnixMillis::EPOCH))
    }

    /// Starts an empty session with canonical creation time and `main` at revision zero.
    #[must_use]
    pub fn with_created_at(session_id: SessionId, created_at_unix_ms: UnixMillis) -> Self {
        Self::with_metadata(SessionMetadata::new(session_id, created_at_unix_ms))
    }

    /// Starts an empty session with canonical metadata and `main` at revision zero.
    #[must_use]
    pub fn with_metadata(metadata: SessionMetadata) -> Self {
        let main = HeadName::new("main")
            .unwrap_or_else(|error| unreachable!("static main head is valid: {error}"));
        Self {
            metadata,
            next_sequence: JournalSequence::new(1),
            records: Vec::new(),
            record_ids: BTreeSet::new(),
            entries: BTreeMap::new(),
            entry_sequences: BTreeMap::new(),
            stable_entries: BTreeSet::new(),
            unstable_entry_turns: BTreeMap::new(),
            heads: BTreeMap::from([(
                main,
                HeadState {
                    target: None,
                    revision: HeadRevision::new(0),
                    open_turn: None,
                },
            )]),
            retired_heads: BTreeSet::new(),
            turn_starts: BTreeMap::new(),
            turn_finishes: BTreeMap::new(),
            model_steps: BTreeSet::new(),
            last_model_step_indexes: BTreeMap::new(),
            request_attempts: BTreeMap::new(),
            request_attempt_order: Vec::new(),
            active_request_owners: BTreeMap::new(),
        }
    }

    /// Session this journal reconstructs.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        self.metadata.session_id()
    }

    /// Wall-clock instant at which this session identity was first created.
    #[must_use]
    pub const fn created_at_unix_ms(&self) -> UnixMillis {
        self.metadata.created_at_unix_ms()
    }

    /// Canonical identity and creation chronology for downstream projections.
    #[must_use]
    pub const fn metadata(&self) -> &SessionMetadata {
        &self.metadata
    }

    /// Exact sequence the next accepted record must carry.
    #[must_use]
    pub const fn next_sequence(&self) -> JournalSequence {
        self.next_sequence
    }

    /// Current revision of a named head.
    pub fn head_revision(&self, head: &HeadName) -> Result<HeadRevision, JournalError> {
        self.head(head).map(|state| state.revision)
    }

    /// Current entry selected by a named head.
    pub fn head_target(&self, head: &HeadName) -> Result<Option<&SessionEntryId>, JournalError> {
        self.head(head).map(|state| state.target.as_ref())
    }

    /// Entries from root through the selected head, in provider order.
    pub fn path(&self, head: &HeadName) -> Result<Vec<&SessionEntry>, JournalError> {
        let mut cursor = self.head(head)?.target.as_ref();
        let mut path = Vec::new();
        while let Some(id) = cursor {
            let entry = self
                .entries
                .get(id)
                .ok_or_else(|| JournalError::MissingEntry(id.clone()))?;
            path.push(entry);
            cursor = entry.parent_id.as_ref();
        }
        path.reverse();
        Ok(path)
    }

    /// Ordered records retained so a file adapter can export or replay them.
    pub fn records(&self) -> &[JournalRecord] {
        &self.records
    }

    /// One request authorization and its optional terminal fact.
    #[must_use]
    pub fn request_attempt(&self, attempt_id: &RequestAttemptId) -> Option<&RequestAttempt> {
        self.request_attempts.get(attempt_id)
    }

    /// Every request attempt in authorization order, including attempts on abandoned heads.
    pub fn request_attempts(&self) -> impl ExactSizeIterator<Item = &RequestAttempt> {
        self.request_attempt_order.iter().map(|attempt_id| {
            self.request_attempts
                .get(attempt_id)
                .unwrap_or_else(|| unreachable!("every ordered attempt remains indexed"))
        })
    }

    /// Applies one already-written record, or leaves the journal byte-for-byte equal on refusal.
    pub fn apply(&mut self, record: JournalRecord) -> Result<(), JournalError> {
        let next_sequence = self.validate(&record)?;
        match &record {
            JournalRecord::AppendEntry {
                sequence,
                head,
                entry,
                ..
            } => {
                let prior_open_turn = self
                    .heads
                    .get(head)
                    .and_then(|state| state.open_turn.clone());
                if let JournalEntryPayload::TurnStarted {
                    agent_id, turn_id, ..
                } = &entry.payload
                {
                    self.turn_starts.insert(
                        turn_id.clone(),
                        TurnStartState {
                            agent_id: agent_id.clone(),
                            entry_id: entry.id.clone(),
                        },
                    );
                }
                if let JournalEntryPayload::AssistantOutput { step_id, .. } = &entry.payload {
                    self.model_steps.insert(step_id.clone());
                    self.last_model_step_indexes
                        .insert(step_id.turn_id().clone(), step_id.index());
                }
                self.entries
                    .insert(entry.id.clone(), entry.as_ref().clone());
                self.entry_sequences.insert(entry.id.clone(), *sequence);
                let next_open_turn = match &entry.payload {
                    JournalEntryPayload::TurnStarted { turn_id, .. } => Some(turn_id.clone()),
                    _ => prior_open_turn,
                };
                if next_open_turn.is_none() {
                    self.stable_entries.insert(entry.id.clone());
                } else if let Some(turn_id) = &next_open_turn {
                    self.unstable_entry_turns
                        .insert(entry.id.clone(), turn_id.clone());
                }
                let state = self
                    .heads
                    .get_mut(head)
                    .unwrap_or_else(|| unreachable!("validated head must remain present"));
                state.target = Some(entry.id.clone());
                state.revision = HeadRevision::new(state.revision.get() + 1);
                state.open_turn = next_open_turn;
            }
            JournalRecord::CreateHead { head, at, .. } => {
                self.heads.insert(
                    head.clone(),
                    HeadState {
                        target: at.clone(),
                        revision: HeadRevision::new(0),
                        open_turn: None,
                    },
                );
            }
            JournalRecord::MoveHead { head, to, .. } => {
                let state = self
                    .heads
                    .get_mut(head)
                    .unwrap_or_else(|| unreachable!("validated head must remain present"));
                state.target = to.clone();
                state.revision = HeadRevision::new(state.revision.get() + 1);
                state.open_turn = None;
            }
            JournalRecord::RenameHead { head, renamed, .. } => {
                let mut state = self
                    .heads
                    .remove(head)
                    .unwrap_or_else(|| unreachable!("validated head must remain present"));
                state.revision = HeadRevision::new(state.revision.get() + 1);
                self.retired_heads.insert(head.clone());
                self.heads.insert(renamed.clone(), state);
            }
            JournalRecord::AbandonHead { head, .. } => {
                self.heads.remove(head);
                self.retired_heads.insert(head.clone());
            }
            JournalRecord::TurnFinished {
                sequence,
                head,
                fact,
                ..
            } => {
                self.turn_finishes.insert(
                    fact.turn_id.clone(),
                    TurnFinishState {
                        sequence: *sequence,
                        fact: fact.clone(),
                    },
                );
                self.stable_entries.insert(fact.semantic_boundary.clone());
                self.unstable_entry_turns.remove(&fact.semantic_boundary);
                self.heads
                    .get_mut(head)
                    .unwrap_or_else(|| unreachable!("validated head remains present"))
                    .open_turn = None;
            }
            JournalRecord::RequestAttemptAuthorized { fact, .. } => {
                self.request_attempt_order.push(fact.attempt_id().clone());
                self.active_request_owners
                    .insert(fact.owner().clone(), fact.attempt_id().clone());
                self.request_attempts.insert(
                    fact.attempt_id().clone(),
                    RequestAttempt::authorized(fact.clone()),
                );
            }
            JournalRecord::RequestAttemptFinished { fact, .. } => {
                let owner = self
                    .request_attempts
                    .get(fact.attempt_id())
                    .unwrap_or_else(|| unreachable!("validated request attempt remains indexed"))
                    .authorization()
                    .owner()
                    .clone();
                self.request_attempts
                    .get_mut(fact.attempt_id())
                    .unwrap_or_else(|| unreachable!("validated request attempt remains indexed"))
                    .finish(fact.clone());
                self.active_request_owners.remove(&owner);
            }
        }
        self.record_ids.insert(record.record_id().clone());
        self.records.push(record);
        self.next_sequence = next_sequence;
        Ok(())
    }

    /// Validates the exact next record without changing this journal.
    pub fn validate_record(&self, record: &JournalRecord) -> Result<(), JournalError> {
        self.validate(record).map(|_| ())
    }

    fn validate(&self, record: &JournalRecord) -> Result<JournalSequence, JournalError> {
        if record.sequence() != self.next_sequence {
            return Err(JournalError::UnexpectedSequence {
                expected: self.next_sequence,
                actual: record.sequence(),
            });
        }
        if self.record_ids.contains(record.record_id()) {
            return Err(JournalError::DuplicateRecord(record.record_id().clone()));
        }
        let next_sequence = self
            .next_sequence
            .get()
            .checked_add(1)
            .map(JournalSequence::new)
            .ok_or(JournalError::SequenceExhausted)?;

        match record {
            JournalRecord::AppendEntry {
                head,
                expected_head_revision,
                entry,
                ..
            } => {
                if self.entries.contains_key(&entry.id) {
                    return Err(JournalError::DuplicateEntry(entry.id.clone()));
                }
                self.validate_target(entry.parent_id.as_ref())?;
                let state = self.validate_head(head, *expected_head_revision)?;
                if entry.parent_id != state.target {
                    return Err(JournalError::ParentMismatch {
                        head: head.clone(),
                        expected: state.target.clone(),
                        actual: entry.parent_id.clone(),
                    });
                }
                match &entry.payload {
                    JournalEntryPayload::AgentCreated {
                        agent_id, status, ..
                    } if *status != plexmaton_core::AgentStatus::Idle => {
                        return Err(JournalError::InvalidInitialAgentStatus(agent_id.clone()));
                    }
                    JournalEntryPayload::TurnStarted { turn_id, .. } => {
                        if self.turn_starts.contains_key(turn_id) {
                            return Err(JournalError::DuplicateTurn(turn_id.clone()));
                        }
                        if let Some(open_turn) = &state.open_turn {
                            return Err(JournalError::UnstableTurnTarget(open_turn.clone()));
                        }
                    }
                    JournalEntryPayload::SteeringAccepted {
                        agent_id, turn_id, ..
                    } => self.validate_steering(agent_id, turn_id, state.open_turn.as_ref())?,
                    JournalEntryPayload::TurnStatusChanged {
                        agent_id, turn_id, ..
                    } => self.validate_turn_status(agent_id, turn_id, state.open_turn.as_ref())?,
                    JournalEntryPayload::AssistantOutput {
                        agent_id, step_id, ..
                    } => {
                        if self.model_steps.contains(step_id) {
                            return Err(JournalError::DuplicateModelStep(step_id.clone()));
                        }
                        let expected = self.expected_model_step_index(step_id.turn_id())?;
                        if step_id.index() != expected {
                            return Err(JournalError::UnexpectedModelStep {
                                turn_id: step_id.turn_id().clone(),
                                expected,
                                actual: step_id.index(),
                            });
                        }
                        self.validate_steering(
                            agent_id,
                            step_id.turn_id(),
                            state.open_turn.as_ref(),
                        )?;
                    }
                    _ => {}
                }
                self.validate_revision_increment(head, state.revision)?;
            }
            JournalRecord::CreateHead { head, at, .. } => {
                self.validate_available_head(head)?;
                self.validate_target(at.as_ref())?;
                self.validate_stable_target(at.as_ref())?;
            }
            JournalRecord::MoveHead {
                head,
                expected_head_revision,
                to,
                ..
            } => {
                let state = self.validate_head(head, *expected_head_revision)?;
                self.validate_target(to.as_ref())?;
                self.validate_stable_target(to.as_ref())?;
                self.validate_revision_increment(head, state.revision)?;
            }
            JournalRecord::RenameHead {
                head,
                expected_head_revision,
                renamed,
                ..
            } => {
                let state = self.validate_head(head, *expected_head_revision)?;
                self.validate_available_head(renamed)?;
                self.validate_revision_increment(head, state.revision)?;
            }
            JournalRecord::AbandonHead {
                head,
                expected_head_revision,
                ..
            } => {
                let state = self.validate_head(head, *expected_head_revision)?;
                self.validate_stable_target(state.target.as_ref())?;
            }
            JournalRecord::TurnFinished {
                head,
                expected_head_revision,
                fact,
                ..
            } => {
                let state = self.validate_head(head, *expected_head_revision)?;
                if state.target.as_ref() != Some(&fact.semantic_boundary) {
                    return Err(JournalError::InvalidTurnBoundary {
                        turn_id: fact.turn_id.clone(),
                        boundary: fact.semantic_boundary.clone(),
                    });
                }
                self.validate_turn_finished(fact, state.open_turn.as_ref())?;
            }
            JournalRecord::RequestAttemptAuthorized {
                head,
                expected_head_revision,
                fact,
                ..
            } => self.validate_request_authorization(head, *expected_head_revision, fact)?,
            JournalRecord::RequestAttemptFinished { fact, .. } => {
                self.validate_request_terminal(fact)?;
            }
        }
        Ok(next_sequence)
    }

    fn head(&self, head: &HeadName) -> Result<&HeadState, JournalError> {
        self.heads
            .get(head)
            .ok_or_else(|| JournalError::MissingHead(head.clone()))
    }

    fn validate_head(
        &self,
        head: &HeadName,
        expected: HeadRevision,
    ) -> Result<&HeadState, JournalError> {
        let state = self.head(head)?;
        if state.revision != expected {
            return Err(JournalError::StaleHead {
                head: head.clone(),
                expected,
                actual: state.revision,
            });
        }
        Ok(state)
    }

    fn validate_available_head(&self, head: &HeadName) -> Result<(), JournalError> {
        if self.heads.contains_key(head) || self.retired_heads.contains(head) {
            return Err(JournalError::UnavailableHeadName(head.clone()));
        }
        Ok(())
    }

    fn validate_target(&self, target: Option<&SessionEntryId>) -> Result<(), JournalError> {
        if let Some(target) = target
            && !self.entries.contains_key(target)
        {
            return Err(JournalError::MissingEntry(target.clone()));
        }
        Ok(())
    }

    fn validate_revision_increment(
        &self,
        head: &HeadName,
        revision: HeadRevision,
    ) -> Result<(), JournalError> {
        revision
            .get()
            .checked_add(1)
            .map(|_| ())
            .ok_or_else(|| JournalError::RevisionExhausted(head.clone()))
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, HeadName, JournalRecordId, SessionEntryId, SessionId, TranscriptItemId, TurnId,
    };

    use super::{
        HeadRevision, JournalEntryPayload, JournalError, JournalRecord, JournalSequence,
        SessionEntry, SessionJournal,
    };
    use crate::MAX_PROVIDER_REPLAY_BYTES;
    use crate::test_support::{
        output, output_with_replay, reasoning_block, replay, step, text_block,
    };
    use crate::{TurnFinished, TurnFinishedAt, TurnOutcome, UnixMillis};

    fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
        build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
    }

    fn session() -> SessionJournal {
        SessionJournal::new(id("session-a", SessionId::new))
    }

    fn head(value: &str) -> HeadName {
        id(value, HeadName::new)
    }

    fn record(value: &str) -> JournalRecordId {
        id(value, JournalRecordId::new)
    }

    fn entry(value: &str, parent_id: Option<SessionEntryId>, text: &str) -> SessionEntry {
        SessionEntry {
            id: id(value, SessionEntryId::new),
            parent_id,
            payload: JournalEntryPayload::RuntimeWarning {
                agent_id: id("agent-a", AgentId::new),
                item_id: id(&format!("item-{value}"), TranscriptItemId::new),
                message: text.to_owned(),
            },
        }
    }

    fn append(
        sequence: u64,
        record_id: &str,
        head: &str,
        revision: u64,
        entry: SessionEntry,
    ) -> JournalRecord {
        JournalRecord::AppendEntry {
            sequence: JournalSequence::new(sequence),
            record_id: record(record_id),
            head: self::head(head),
            expected_head_revision: HeadRevision::new(revision),
            entry: Box::new(entry),
        }
    }

    fn turn_entry(
        value: &str,
        parent_id: Option<SessionEntryId>,
        turn_id: &str,
        agent_id: &str,
        text: &str,
    ) -> SessionEntry {
        SessionEntry {
            id: id(value, SessionEntryId::new),
            parent_id,
            payload: JournalEntryPayload::TurnStarted {
                agent_id: id(agent_id, AgentId::new),
                item_id: id(&format!("item-{value}"), TranscriptItemId::new),
                turn_id: id(turn_id, TurnId::new),
                text: text.to_owned(),
                accepted_at: UnixMillis::new(10),
                opened_at: UnixMillis::new(12),
            },
        }
    }

    fn finish_record(
        journal: &SessionJournal,
        record_id: &str,
        turn_id: &str,
        agent_id: &str,
        boundary: &str,
    ) -> JournalRecord {
        JournalRecord::TurnFinished {
            sequence: journal.next_sequence(),
            record_id: record(record_id),
            head: head("main"),
            expected_head_revision: journal
                .head_revision(&head("main"))
                .unwrap_or_else(|error| panic!("head revision: {error:?}")),
            fact: TurnFinished {
                agent_id: id(agent_id, AgentId::new),
                turn_id: id(turn_id, TurnId::new),
                semantic_boundary: id(boundary, SessionEntryId::new),
                outcome: TurnOutcome::Completed,
                at: TurnFinishedAt::Observed {
                    completed_at: UnixMillis::new(20),
                },
            },
        }
    }

    /// JRN-1: content append and head advance are one compare-and-set record.
    #[test]
    fn jrn_1_append_and_head_mutations_form_one_checked_tree() {
        let mut journal = session();
        let first = entry("entry-1", None, "root");
        journal
            .apply(append(1, "record-1", "main", 0, first.clone()))
            .unwrap_or_else(|error| panic!("append root: {error:?}"));
        journal
            .apply(JournalRecord::CreateHead {
                sequence: JournalSequence::new(2),
                record_id: record("record-2"),
                head: head("experiment"),
                at: Some(first.id.clone()),
            })
            .unwrap_or_else(|error| panic!("create head: {error:?}"));
        let second = entry("entry-2", Some(first.id.clone()), "branch");
        journal
            .apply(append(3, "record-3", "experiment", 0, second.clone()))
            .unwrap_or_else(|error| panic!("append branch: {error:?}"));
        journal
            .apply(JournalRecord::MoveHead {
                sequence: JournalSequence::new(4),
                record_id: record("record-4"),
                head: head("experiment"),
                expected_head_revision: HeadRevision::new(1),
                to: Some(first.id.clone()),
            })
            .unwrap_or_else(|error| panic!("rewind head: {error:?}"));
        journal
            .apply(JournalRecord::RenameHead {
                sequence: JournalSequence::new(5),
                record_id: record("record-5"),
                head: head("experiment"),
                expected_head_revision: HeadRevision::new(2),
                renamed: head("kept"),
            })
            .unwrap_or_else(|error| panic!("rename head: {error:?}"));
        journal
            .apply(JournalRecord::AbandonHead {
                sequence: JournalSequence::new(6),
                record_id: record("record-6"),
                head: head("kept"),
                expected_head_revision: HeadRevision::new(3),
            })
            .unwrap_or_else(|error| panic!("abandon head: {error:?}"));

        assert_eq!(journal.head_target(&head("main")), Ok(Some(&first.id)));
        assert_eq!(
            journal.head_revision(&head("main")),
            Ok(HeadRevision::new(1))
        );
        assert_eq!(
            journal.head_target(&head("kept")),
            Err(JournalError::MissingHead(head("kept")))
        );
        assert_eq!(journal.records().len(), 6);
    }

    /// TIM-1/TIM-4: terminal chronology is canonical without becoming semantic ancestry.
    #[test]
    fn tim_1_turn_terminal_round_trips_without_advancing_its_head() {
        let mut journal = session();
        let started = turn_entry("turn-entry", None, "turn-1", "agent-a", "hello");
        journal
            .apply(append(1, "record-1", "main", 0, started.clone()))
            .unwrap_or_else(|error| panic!("start turn: {error:?}"));
        let before_target = journal
            .head_target(&head("main"))
            .map(|target| target.cloned());
        let before_revision = journal.head_revision(&head("main"));
        let finished = finish_record(&journal, "record-2", "turn-1", "agent-a", "turn-entry");
        let json = serde_json::to_string(&finished)
            .unwrap_or_else(|error| panic!("encode terminal: {error}"));
        let decoded: JournalRecord =
            serde_json::from_str(&json).unwrap_or_else(|error| panic!("decode terminal: {error}"));
        assert_eq!(decoded, finished);
        journal
            .apply(finished)
            .unwrap_or_else(|error| panic!("finish turn: {error:?}"));

        assert_eq!(
            journal
                .head_target(&head("main"))
                .map(|target| target.cloned()),
            before_target
        );
        assert_eq!(journal.head_revision(&head("main")), before_revision);
        assert_eq!(journal.path(&head("main")), Ok(vec![&started]));
    }

    fn open_turn_fixture() -> (SessionJournal, SessionEntry) {
        let mut journal = session();
        let started = turn_entry("turn-entry", None, "turn-1", "agent-a", "hello");
        journal
            .apply(append(1, "record-1", "main", 0, started.clone()))
            .unwrap_or_else(|error| panic!("start turn: {error:?}"));
        (journal, started)
    }

    /// TIM-1/JRN-2: branch operations cannot select a partial turn, while rename retains it.
    #[test]
    fn tim_1_partial_head_mutations_preserve_or_refuse_the_open_turn() {
        let (mut journal, started) = open_turn_fixture();
        let open = journal.clone();
        let invalid = [
            JournalRecord::CreateHead {
                sequence: journal.next_sequence(),
                record_id: record("partial-create"),
                head: head("partial"),
                at: Some(started.id.clone()),
            },
            JournalRecord::MoveHead {
                sequence: journal.next_sequence(),
                record_id: record("partial-move"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(1),
                to: Some(started.id.clone()),
            },
            JournalRecord::AbandonHead {
                sequence: journal.next_sequence(),
                record_id: record("partial-abandon"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(1),
            },
        ];
        for record in invalid {
            assert!(journal.apply(record).is_err());
            assert_eq!(journal, open);
        }

        journal
            .apply(JournalRecord::RenameHead {
                sequence: JournalSequence::new(2),
                record_id: record("partial-rename"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(1),
                renamed: head("continued"),
            })
            .unwrap_or_else(|error| panic!("rename partial head: {error:?}"));
        journal
            .apply(JournalRecord::TurnFinished {
                sequence: JournalSequence::new(3),
                record_id: record("finish-renamed"),
                head: head("continued"),
                expected_head_revision: HeadRevision::new(2),
                fact: TurnFinished {
                    agent_id: id("agent-a", AgentId::new),
                    turn_id: id("turn-1", TurnId::new),
                    semantic_boundary: started.id,
                    outcome: TurnOutcome::Completed,
                    at: TurnFinishedAt::Observed {
                        completed_at: UnixMillis::new(30),
                    },
                },
            })
            .unwrap_or_else(|error| panic!("finish renamed turn: {error:?}"));
    }

    /// TIM-1/JRN-2: malformed terminal facts mutate nothing and a turn finishes once.
    #[test]
    fn tim_1_invalid_terminal_records_change_nothing() {
        let (mut journal, started) = open_turn_fixture();
        let open = journal.clone();
        let invalid = [
            finish_record(&journal, "wrong-owner", "turn-1", "agent-b", "turn-entry"),
            finish_record(
                &journal,
                "missing-turn",
                "turn-missing",
                "agent-a",
                "turn-entry",
            ),
            JournalRecord::TurnFinished {
                sequence: journal.next_sequence(),
                record_id: record("missing-head"),
                head: head("missing"),
                expected_head_revision: HeadRevision::new(1),
                fact: TurnFinished {
                    agent_id: id("agent-a", AgentId::new),
                    turn_id: id("turn-1", TurnId::new),
                    semantic_boundary: started.id.clone(),
                    outcome: TurnOutcome::Completed,
                    at: TurnFinishedAt::Observed {
                        completed_at: UnixMillis::new(20),
                    },
                },
            },
            JournalRecord::TurnFinished {
                sequence: journal.next_sequence(),
                record_id: record("stale-head"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(0),
                fact: TurnFinished {
                    agent_id: id("agent-a", AgentId::new),
                    turn_id: id("turn-1", TurnId::new),
                    semantic_boundary: started.id.clone(),
                    outcome: TurnOutcome::Completed,
                    at: TurnFinishedAt::Observed {
                        completed_at: UnixMillis::new(20),
                    },
                },
            },
            JournalRecord::TurnFinished {
                sequence: journal.next_sequence(),
                record_id: record("bad-boundary"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(1),
                fact: TurnFinished {
                    agent_id: id("agent-a", AgentId::new),
                    turn_id: id("turn-1", TurnId::new),
                    semantic_boundary: id("outside-turn", SessionEntryId::new),
                    outcome: TurnOutcome::Completed,
                    at: TurnFinishedAt::Observed {
                        completed_at: UnixMillis::new(20),
                    },
                },
            },
            JournalRecord::TurnFinished {
                sequence: journal.next_sequence(),
                record_id: record("observed-process-death"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(1),
                fact: TurnFinished {
                    agent_id: id("agent-a", AgentId::new),
                    turn_id: id("turn-1", TurnId::new),
                    semantic_boundary: started.id.clone(),
                    outcome: TurnOutcome::ProcessDied,
                    at: TurnFinishedAt::Observed {
                        completed_at: UnixMillis::new(20),
                    },
                },
            },
            JournalRecord::TurnFinished {
                sequence: journal.next_sequence(),
                record_id: record("recovered-completed"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(1),
                fact: TurnFinished {
                    agent_id: id("agent-a", AgentId::new),
                    turn_id: id("turn-1", TurnId::new),
                    semantic_boundary: started.id.clone(),
                    outcome: TurnOutcome::Completed,
                    at: TurnFinishedAt::Recovered {
                        recovery_observed_at: UnixMillis::new(20),
                    },
                },
            },
        ];
        for record in invalid {
            assert!(journal.apply(record).is_err());
            assert_eq!(journal, open);
        }

        let finished = finish_record(&journal, "valid-finish", "turn-1", "agent-a", "turn-entry");
        journal
            .apply(finished)
            .unwrap_or_else(|error| panic!("valid finish: {error:?}"));
        let closed = journal.clone();
        assert!(
            journal
                .apply(finish_record(
                    &journal,
                    "duplicate-finish",
                    "turn-1",
                    "agent-a",
                    "turn-entry",
                ))
                .is_err()
        );
        assert_eq!(journal, closed);
    }

    /// TIM-1/TIM-4: sibling branches share completed ancestry but never one another's later audit.
    #[test]
    fn tim_1_sibling_heads_project_only_their_own_later_turns_and_terminals() {
        let mut journal = session();
        let created = entry("created", None, "ignored");
        let created = SessionEntry {
            payload: JournalEntryPayload::AgentCreated {
                agent_id: id("agent-a", AgentId::new),
                label: "Agent A".to_owned(),
                status: plexmaton_core::AgentStatus::Idle,
            },
            ..created
        };
        journal
            .apply(append(1, "record-1", "main", 0, created.clone()))
            .unwrap_or_else(|error| panic!("announce: {error:?}"));
        let shared = turn_entry(
            "shared",
            Some(created.id.clone()),
            "turn-shared",
            "agent-a",
            "shared",
        );
        journal
            .apply(append(2, "record-2", "main", 1, shared.clone()))
            .unwrap_or_else(|error| panic!("shared start: {error:?}"));
        journal
            .apply(finish_record(
                &journal,
                "record-3",
                "turn-shared",
                "agent-a",
                "shared",
            ))
            .unwrap_or_else(|error| panic!("shared finish: {error:?}"));
        journal
            .apply(JournalRecord::CreateHead {
                sequence: JournalSequence::new(4),
                record_id: record("record-4"),
                head: head("branch"),
                at: Some(shared.id.clone()),
            })
            .unwrap_or_else(|error| panic!("create sibling: {error:?}"));

        let main = turn_entry(
            "main-turn",
            Some(shared.id.clone()),
            "turn-main",
            "agent-a",
            "main",
        );
        journal
            .apply(append(5, "record-5", "main", 2, main))
            .unwrap_or_else(|error| panic!("main start: {error:?}"));
        journal
            .apply(finish_record(
                &journal,
                "record-6",
                "turn-main",
                "agent-a",
                "main-turn",
            ))
            .unwrap_or_else(|error| panic!("main finish: {error:?}"));

        let branch = turn_entry(
            "branch-turn",
            Some(shared.id),
            "turn-branch",
            "agent-a",
            "branch",
        );
        journal
            .apply(append(7, "record-7", "branch", 0, branch))
            .unwrap_or_else(|error| panic!("branch start: {error:?}"));
        journal
            .apply(JournalRecord::TurnFinished {
                sequence: JournalSequence::new(8),
                record_id: record("record-8"),
                head: head("branch"),
                expected_head_revision: HeadRevision::new(1),
                fact: TurnFinished {
                    agent_id: id("agent-a", AgentId::new),
                    turn_id: id("turn-branch", TurnId::new),
                    semantic_boundary: id("branch-turn", SessionEntryId::new),
                    outcome: TurnOutcome::Completed,
                    at: TurnFinishedAt::Observed {
                        completed_at: UnixMillis::new(30),
                    },
                },
            })
            .unwrap_or_else(|error| panic!("branch finish: {error:?}"));

        for (head_name, expected) in [
            ("main", ["shared", "main"]),
            ("branch", ["shared", "branch"]),
        ] {
            let projection = journal
                .project(&head(head_name))
                .unwrap_or_else(|error| panic!("project {head_name}: {error:?}"));
            assert_eq!(
                projection
                    .request()
                    .atoms
                    .iter()
                    .filter_map(|atom| match atom.value() {
                        crate::ContextAtomValue::User { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                projection
                    .events()
                    .iter()
                    .filter(|event| matches!(
                        event.event,
                        plexmaton_core::SessionEvent::AgentStatusChanged {
                            status: plexmaton_core::AgentStatus::Idle,
                            ..
                        }
                    ))
                    .count(),
                2,
                "{head_name} received its sibling's terminal audit"
            );
        }
    }

    /// TIM-1: long linear reload retains one derived stability lookup per terminal boundary.
    #[test]
    fn tim_1_long_turn_history_reduces_with_indexed_stability() {
        let mut journal = session();
        for ordinal in 1..=1_024_u64 {
            let parent = journal
                .head_target(&head("main"))
                .unwrap_or_else(|error| panic!("head target: {error:?}"))
                .cloned();
            let entry_name = format!("entry-{ordinal}");
            let turn_name = format!("turn-{ordinal}");
            let started = turn_entry(&entry_name, parent, &turn_name, "agent-a", "hello");
            let sequence = journal.next_sequence();
            let revision = journal
                .head_revision(&head("main"))
                .unwrap_or_else(|error| panic!("head revision: {error:?}"));
            journal
                .apply(append(
                    sequence.get(),
                    &format!("start-{ordinal}"),
                    "main",
                    revision.get(),
                    started,
                ))
                .unwrap_or_else(|error| panic!("start turn {ordinal}: {error:?}"));
            let finished = finish_record(
                &journal,
                &format!("finish-{ordinal}"),
                &turn_name,
                "agent-a",
                &entry_name,
            );
            journal
                .apply(finished)
                .unwrap_or_else(|error| panic!("finish turn {ordinal}: {error:?}"));
        }

        let parent = journal
            .head_target(&head("main"))
            .unwrap_or_else(|error| panic!("head target: {error:?}"))
            .cloned();
        journal
            .apply(append(
                journal.next_sequence().get(),
                "start-open-tail",
                "main",
                journal
                    .head_revision(&head("main"))
                    .unwrap_or_else(|error| panic!("head revision: {error:?}"))
                    .get(),
                turn_entry(
                    "entry-open-tail",
                    parent,
                    "turn-open-tail",
                    "agent-a",
                    "still running",
                ),
            ))
            .unwrap_or_else(|error| panic!("start open tail: {error:?}"));
        let open = journal.clone();
        assert_eq!(
            journal.apply(JournalRecord::CreateHead {
                sequence: journal.next_sequence(),
                record_id: record("branch-open-tail"),
                head: head("invalid-branch"),
                at: journal
                    .head_target(&head("main"))
                    .unwrap_or_else(|error| panic!("head target: {error:?}"))
                    .cloned(),
            }),
            Err(JournalError::UnstableTurnTarget(id(
                "turn-open-tail",
                TurnId::new
            )))
        );
        assert_eq!(journal, open);
        assert_eq!(journal.turn_starts.len(), 1_025);
        assert_eq!(journal.turn_finishes.len(), 1_024);
        assert_eq!(journal.stable_entries.len(), 1_024);
        assert_eq!(journal.unstable_entry_turns.len(), 1);
        assert_eq!(
            journal.open_turn_on_path(&head("main")),
            Some(id("turn-open-tail", TurnId::new))
        );
    }

    /// JRN-2: every refusal is transactional at the reducer boundary.
    #[test]
    fn jrn_2_invalid_records_change_nothing() {
        let mut journal = session();
        let root = entry("entry-1", None, "root");
        journal
            .apply(append(1, "record-1", "main", 0, root.clone()))
            .unwrap_or_else(|error| panic!("append root: {error:?}"));
        let unchanged = journal.clone();

        let invalid = [
            (
                append(2, "record-2", "main", 0, entry("entry-2", None, "stale")),
                "stale head revision",
            ),
            (
                append(
                    3,
                    "record-3",
                    "main",
                    1,
                    entry("entry-3", Some(root.id.clone()), "gap"),
                ),
                "sequence gap",
            ),
            (
                append(
                    2,
                    "record-1",
                    "main",
                    1,
                    entry("entry-4", Some(root.id.clone()), "duplicate record"),
                ),
                "duplicate record identity",
            ),
            (
                append(2, "record-4", "main", 1, root.clone()),
                "duplicate entry identity",
            ),
            (
                append(
                    2,
                    "record-5",
                    "main",
                    1,
                    entry("entry-5", None, "wrong parent"),
                ),
                "parent mismatch",
            ),
            (
                JournalRecord::MoveHead {
                    sequence: JournalSequence::new(2),
                    record_id: record("record-6"),
                    head: head("main"),
                    expected_head_revision: HeadRevision::new(1),
                    to: Some(id("missing", SessionEntryId::new)),
                },
                "missing target",
            ),
        ];

        for (record, case) in invalid {
            assert!(journal.apply(record).is_err(), "accepted {case}");
            assert_eq!(journal, unchanged, "{case} changed the journal");
        }
    }

    /// JRN-2: a turn's model-step chronology is contiguous and one-based.
    #[test]
    fn jrn_2_model_steps_cannot_skip_rewind_or_repeat() {
        let mut journal = session();
        let turn = turn_entry("turn-entry", None, "turn-a", "agent-a", "hello");
        let turn_entry_id = turn.id.clone();
        journal
            .apply(append(1, "turn-record", "main", 0, turn))
            .unwrap_or_else(|error| panic!("start turn: {error:?}"));
        let step_entry = |entry_id: &str, parent_id: SessionEntryId, index| SessionEntry {
            id: id(entry_id, SessionEntryId::new),
            parent_id: Some(parent_id),
            payload: JournalEntryPayload::AssistantOutput {
                agent_id: id("agent-a", AgentId::new),
                step_id: step("turn-a", index),
                output: output(vec![text_block(&format!("text-{entry_id}"), "assistant")]),
            },
        };

        let after_turn = journal.clone();
        assert_eq!(
            journal.apply(append(
                2,
                "step-two-first",
                "main",
                1,
                step_entry("step-two-entry", turn_entry_id.clone(), 2),
            )),
            Err(JournalError::UnexpectedModelStep {
                turn_id: id("turn-a", TurnId::new),
                expected: 1,
                actual: 2,
            })
        );
        assert_eq!(journal, after_turn);

        let first = step_entry("step-one-entry", turn_entry_id, 1);
        let first_id = first.id.clone();
        journal
            .apply(append(2, "step-one", "main", 1, first))
            .unwrap_or_else(|error| panic!("append first step: {error:?}"));
        let after_first = journal.clone();
        assert!(matches!(
            journal.apply(append(
                3,
                "step-three",
                "main",
                2,
                step_entry("step-three-entry", first_id.clone(), 3),
            )),
            Err(JournalError::UnexpectedModelStep {
                expected: 2,
                actual: 3,
                ..
            })
        ));
        assert_eq!(journal, after_first);
        assert!(matches!(
            journal.apply(append(
                3,
                "step-one-again",
                "main",
                2,
                step_entry("step-one-again-entry", first_id, 1),
            )),
            Err(JournalError::DuplicateModelStep(_))
        ));
        assert_eq!(journal, after_first);
    }

    /// JRN-2: live and replay reduction have exactly one result.
    #[test]
    fn jrn_2_the_same_records_build_equal_journals_and_paths() {
        let root = entry("entry-1", None, "root");
        let child = entry("entry-2", Some(root.id.clone()), "child");
        let records = vec![
            append(1, "record-1", "main", 0, root),
            append(2, "record-2", "main", 1, child),
        ];
        let mut left = session();
        let mut right = session();
        for record in records {
            left.apply(record.clone())
                .unwrap_or_else(|error| panic!("left replay: {error:?}"));
            right
                .apply(record)
                .unwrap_or_else(|error| panic!("right replay: {error:?}"));
        }

        assert_eq!(left, right);
        let texts: Vec<_> = left
            .path(&head("main"))
            .unwrap_or_else(|error| panic!("main path: {error:?}"))
            .iter()
            .filter_map(|entry| match &entry.payload {
                JournalEntryPayload::RuntimeWarning { message, .. } => Some(message.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["root", "child"]);
    }

    /// JRN-3: lossless storage may contain opaque replay while ordinary inspection cannot.
    #[test]
    fn jrn_3_every_record_round_trips_and_debug_redacts_replay() {
        let replay = replay(r#"{"type":"reasoning","encrypted_content":"secret-ciphertext"}"#);
        let record = append(
            1,
            "record-1",
            "main",
            0,
            SessionEntry {
                id: id("entry-1", SessionEntryId::new),
                parent_id: None,
                payload: JournalEntryPayload::AssistantOutput {
                    agent_id: id("agent-a", AgentId::new),
                    step_id: step("turn-1", 1),
                    output: output_with_replay(
                        vec![reasoning_block("reasoning-1", "bounded reasoning")],
                        [(0, replay)],
                    ),
                },
            },
        );

        let encoded = serde_json::to_string(&record)
            .unwrap_or_else(|error| panic!("encode journal record: {error}"));
        assert!(encoded.contains("secret-ciphertext"));
        let decoded: JournalRecord = serde_json::from_str(&encoded)
            .unwrap_or_else(|error| panic!("decode journal record: {error}"));
        assert_eq!(decoded, record);
        assert!(!format!("{record:?}").contains("secret-ciphertext"));

        let variants = [
            JournalRecord::CreateHead {
                sequence: JournalSequence::new(1),
                record_id: self::record("create"),
                head: head("branch"),
                at: None,
            },
            JournalRecord::MoveHead {
                sequence: JournalSequence::new(1),
                record_id: self::record("move"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(0),
                to: None,
            },
            JournalRecord::RenameHead {
                sequence: JournalSequence::new(1),
                record_id: self::record("rename"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(0),
                renamed: head("renamed"),
            },
            JournalRecord::AbandonHead {
                sequence: JournalSequence::new(1),
                record_id: self::record("abandon"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(0),
            },
            JournalRecord::TurnFinished {
                sequence: JournalSequence::new(1),
                record_id: self::record("turn-finished"),
                head: head("main"),
                expected_head_revision: HeadRevision::new(0),
                fact: TurnFinished {
                    agent_id: id("agent-a", AgentId::new),
                    turn_id: id("turn-1", TurnId::new),
                    semantic_boundary: id("entry-1", SessionEntryId::new),
                    outcome: TurnOutcome::Completed,
                    at: TurnFinishedAt::Observed {
                        completed_at: UnixMillis::new(123),
                    },
                },
            },
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant)
                .unwrap_or_else(|error| panic!("encode variant: {error}"));
            let decoded = serde_json::from_str::<JournalRecord>(&json)
                .unwrap_or_else(|error| panic!("decode variant: {error}"));
            assert_eq!(decoded, variant);
        }
    }

    /// JRN-3: decoding cannot bypass constructors at the storage boundary.
    #[test]
    fn jrn_3_decoding_rechecks_identity_and_replay_bounds() {
        let valid = serde_json::to_value(append(
            1,
            "record-1",
            "main",
            0,
            SessionEntry {
                id: id("entry-1", SessionEntryId::new),
                parent_id: None,
                payload: JournalEntryPayload::AssistantOutput {
                    agent_id: id("agent-a", AgentId::new),
                    step_id: step("turn-1", 1),
                    output: output_with_replay(
                        vec![reasoning_block("reasoning-1", "bounded reasoning")],
                        [(0, replay("ciphertext"))],
                    ),
                },
            },
        ))
        .unwrap_or_else(|error| panic!("encode fixture: {error}"));

        let mut empty_id = valid.clone();
        empty_id["record_id"] = serde_json::Value::String(String::new());
        assert!(serde_json::from_value::<JournalRecord>(empty_id).is_err());

        let mut oversized = valid;
        oversized["entry"]["payload"]["output"]["replay"]["attachments"][0]["payload"] =
            serde_json::Value::String("x".repeat(MAX_PROVIDER_REPLAY_BYTES + 1));
        let error = match serde_json::from_value::<JournalRecord>(oversized) {
            Ok(_) => panic!("oversized replay decoded"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("byte bound"));
        assert!(!error.contains(&"x".repeat(64)));
    }
}
