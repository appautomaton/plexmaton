//! The canonical session record's structural core.
//!
//! This module owns no file. A later adapter appends one prepared [`JournalRecord`] and only then
//! gives that same value to [`SessionJournal::apply`]. Keeping preparation and reduction pure makes
//! a JSONL reload the same operation as a live append (JRN-1, JRN-2).

use std::collections::{BTreeMap, BTreeSet};

use plexmaton_core::{HeadName, JournalRecordId, SessionEntryId, SessionId};

mod error;
mod payload;
#[cfg(test)]
mod payload_tests;
mod projection;
mod record;
#[cfg(test)]
mod validation_tests;

pub use error::JournalError;
pub use payload::JournalEntryPayload;
pub use projection::{JournalProjection, JournalProjectionError, RecoveryProjection};
pub use record::{HeadRevision, JournalRecord, JournalSequence, SessionEntry};

#[derive(Clone, Debug, Eq, PartialEq)]
struct HeadState {
    target: Option<SessionEntryId>,
    revision: HeadRevision,
}

/// Deterministic in-memory reduction of one session's ordered records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionJournal {
    session_id: SessionId,
    next_sequence: JournalSequence,
    records: Vec<JournalRecord>,
    record_ids: BTreeSet<JournalRecordId>,
    entries: BTreeMap<SessionEntryId, SessionEntry>,
    heads: BTreeMap<HeadName, HeadState>,
    retired_heads: BTreeSet<HeadName>,
}

impl SessionJournal {
    /// Starts an empty session with `main` at revision zero.
    #[must_use]
    pub fn new(session_id: SessionId) -> Self {
        let main = HeadName::new("main")
            .unwrap_or_else(|error| unreachable!("static main head is valid: {error}"));
        Self {
            session_id,
            next_sequence: JournalSequence::new(1),
            records: Vec::new(),
            record_ids: BTreeSet::new(),
            entries: BTreeMap::new(),
            heads: BTreeMap::from([(
                main,
                HeadState {
                    target: None,
                    revision: HeadRevision::new(0),
                },
            )]),
            retired_heads: BTreeSet::new(),
        }
    }

    /// Session this journal reconstructs.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
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

    /// Applies one already-written record, or leaves the journal byte-for-byte equal on refusal.
    pub fn apply(&mut self, record: JournalRecord) -> Result<(), JournalError> {
        let next_sequence = self.validate(&record)?;
        match &record {
            JournalRecord::AppendEntry { head, entry, .. } => {
                self.entries
                    .insert(entry.id.clone(), entry.as_ref().clone());
                let state = self
                    .heads
                    .get_mut(head)
                    .unwrap_or_else(|| unreachable!("validated head must remain present"));
                state.target = Some(entry.id.clone());
                state.revision = HeadRevision::new(state.revision.get() + 1);
            }
            JournalRecord::CreateHead { head, at, .. } => {
                self.heads.insert(
                    head.clone(),
                    HeadState {
                        target: at.clone(),
                        revision: HeadRevision::new(0),
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
                self.validate_revision_increment(head, state.revision)?;
            }
            JournalRecord::CreateHead { head, at, .. } => {
                self.validate_available_head(head)?;
                self.validate_target(at.as_ref())?;
            }
            JournalRecord::MoveHead {
                head,
                expected_head_revision,
                to,
                ..
            } => {
                let state = self.validate_head(head, *expected_head_revision)?;
                self.validate_target(to.as_ref())?;
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
                self.validate_head(head, *expected_head_revision)?;
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
        AgentId, HeadName, JournalRecordId, SessionEntryId, SessionId, TranscriptItemId,
        TranscriptRole,
    };

    use super::{
        HeadRevision, JournalEntryPayload, JournalError, JournalRecord, JournalSequence,
        SessionEntry, SessionJournal,
    };
    use crate::{MAX_PROVIDER_REPLAY_BYTES, ProviderCodecId, ProviderReplay};

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
            payload: JournalEntryPayload::Message {
                agent_id: id("agent-a", AgentId::new),
                item_id: id(&format!("item-{value}"), TranscriptItemId::new),
                role: TranscriptRole::User,
                text: text.to_owned(),
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
                JournalEntryPayload::Message {
                    role: TranscriptRole::User,
                    text,
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["root", "child"]);
    }

    /// JRN-3: lossless storage may contain opaque replay while ordinary inspection cannot.
    #[test]
    fn jrn_3_every_record_round_trips_and_debug_redacts_replay() {
        let replay = ProviderReplay::new(
            ProviderCodecId::new("openai_responses")
                .unwrap_or_else(|error| panic!("fixture codec: {error:?}")),
            r#"{"type":"reasoning","encrypted_content":"secret-ciphertext"}"#.to_owned(),
        )
        .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
        let record = append(
            1,
            "record-1",
            "main",
            0,
            SessionEntry {
                id: id("entry-1", SessionEntryId::new),
                parent_id: None,
                payload: JournalEntryPayload::ProviderReplay(replay),
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
                payload: JournalEntryPayload::ProviderReplay(
                    ProviderReplay::new(
                        ProviderCodecId::new("openai_responses")
                            .unwrap_or_else(|error| panic!("fixture codec: {error:?}")),
                        "ciphertext".to_owned(),
                    )
                    .unwrap_or_else(|error| panic!("fixture replay: {error:?}")),
                ),
            },
        ))
        .unwrap_or_else(|error| panic!("encode fixture: {error}"));

        let mut empty_id = valid.clone();
        empty_id["record_id"] = serde_json::Value::String(String::new());
        assert!(serde_json::from_value::<JournalRecord>(empty_id).is_err());

        let mut oversized = valid;
        oversized["entry"]["payload"]["payload"] =
            serde_json::Value::String("x".repeat(MAX_PROVIDER_REPLAY_BYTES + 1));
        let error = match serde_json::from_value::<JournalRecord>(oversized) {
            Ok(_) => panic!("oversized replay decoded"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("byte bound"));
        assert!(!error.contains(&"x".repeat(64)));
    }
}
