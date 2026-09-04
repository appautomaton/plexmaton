use plexmaton_core::{
    AgentId, HeadName, JournalRecordId, SessionEntryId, SessionId, ToolCallId, ToolPresentation,
    TranscriptItemId, TranscriptRole,
};

use super::{
    HeadRevision, JournalEntryPayload, JournalError, JournalRecord, JournalSequence, SessionEntry,
    SessionJournal,
};
use crate::{AdmissionRefusal, ToolCall, ToolCancellationReason, ToolOutcome};

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn head(value: &str) -> HeadName {
    id(value, HeadName::new)
}

fn record(value: &str) -> JournalRecordId {
    id(value, JournalRecordId::new)
}

fn entry(value: &str, parent_id: Option<SessionEntryId>) -> SessionEntry {
    SessionEntry {
        id: id(value, SessionEntryId::new),
        parent_id,
        payload: JournalEntryPayload::Message {
            agent_id: id("agent-a", AgentId::new),
            item_id: id(&format!("item-{value}"), TranscriptItemId::new),
            role: TranscriptRole::User,
            text: value.to_owned(),
        },
    }
}

fn append(
    sequence: u64,
    record_id: &str,
    head_name: &str,
    revision: u64,
    entry: SessionEntry,
) -> JournalRecord {
    JournalRecord::AppendEntry {
        sequence: JournalSequence::new(sequence),
        record_id: record(record_id),
        head: head(head_name),
        expected_head_revision: HeadRevision::new(revision),
        entry: Box::new(entry),
    }
}

fn rooted() -> (SessionJournal, SessionEntryId) {
    let mut journal = SessionJournal::new(id("session-a", SessionId::new));
    let root = entry("entry-1", None);
    let root_id = root.id.clone();
    journal
        .apply(append(1, "record-1", "main", 0, root))
        .unwrap_or_else(|error| panic!("append root: {error:?}"));
    (journal, root_id)
}

/// JRN-2: each mutation arm wires its named head through revision validation.
#[test]
fn jrn_2_each_head_mutation_rejects_a_stale_revision() {
    let (journal, root_id) = rooted();
    let stale = [
        append(
            2,
            "append-stale",
            "main",
            0,
            entry("entry-2", Some(root_id.clone())),
        ),
        JournalRecord::MoveHead {
            sequence: JournalSequence::new(2),
            record_id: record("move-stale"),
            head: head("main"),
            expected_head_revision: HeadRevision::new(0),
            to: Some(root_id),
        },
        JournalRecord::RenameHead {
            sequence: JournalSequence::new(2),
            record_id: record("rename-stale"),
            head: head("main"),
            expected_head_revision: HeadRevision::new(0),
            renamed: head("renamed"),
        },
        JournalRecord::AbandonHead {
            sequence: JournalSequence::new(2),
            record_id: record("abandon-stale"),
            head: head("main"),
            expected_head_revision: HeadRevision::new(0),
        },
    ];

    for mutation in stale {
        let mut attempt = journal.clone();
        assert!(matches!(
            attempt.apply(mutation),
            Err(JournalError::StaleHead { .. })
        ));
        assert_eq!(attempt, journal);
    }
}

/// JRN-2: each mutation arm refuses a head it cannot resolve.
#[test]
fn jrn_2_each_head_mutation_rejects_a_missing_head() {
    let (journal, root_id) = rooted();
    let missing = [
        append(
            2,
            "append-missing",
            "missing",
            0,
            entry("entry-2", Some(root_id.clone())),
        ),
        JournalRecord::MoveHead {
            sequence: JournalSequence::new(2),
            record_id: record("move-missing"),
            head: head("missing"),
            expected_head_revision: HeadRevision::new(0),
            to: Some(root_id),
        },
        JournalRecord::RenameHead {
            sequence: JournalSequence::new(2),
            record_id: record("rename-missing"),
            head: head("missing"),
            expected_head_revision: HeadRevision::new(0),
            renamed: head("renamed"),
        },
        JournalRecord::AbandonHead {
            sequence: JournalSequence::new(2),
            record_id: record("abandon-missing"),
            head: head("missing"),
            expected_head_revision: HeadRevision::new(0),
        },
    ];

    for mutation in missing {
        let mut attempt = journal.clone();
        assert_eq!(
            attempt.apply(mutation),
            Err(JournalError::MissingHead(head("missing")))
        );
        assert_eq!(attempt, journal);
    }
}

/// JRN-2: unknown ancestry is distinct from diverging known ancestry.
#[test]
fn jrn_2_an_unknown_append_parent_is_a_missing_entry() {
    let (mut journal, _) = rooted();
    let unchanged = journal.clone();
    let missing = id("missing", SessionEntryId::new);
    let result = journal.apply(append(
        2,
        "record-2",
        "main",
        1,
        entry("entry-2", Some(missing.clone())),
    ));

    assert_eq!(result, Err(JournalError::MissingEntry(missing)));
    assert_eq!(journal, unchanged);
}

/// JRN-2: active and retired names cannot be rebound to unrelated ancestry.
#[test]
fn jrn_2_head_names_are_never_reused() {
    let (mut journal, root_id) = rooted();
    let active = JournalRecord::CreateHead {
        sequence: JournalSequence::new(2),
        record_id: record("record-active"),
        head: head("main"),
        at: Some(root_id.clone()),
    };
    assert_eq!(
        journal.apply(active),
        Err(JournalError::UnavailableHeadName(head("main")))
    );

    journal
        .apply(JournalRecord::AbandonHead {
            sequence: JournalSequence::new(2),
            record_id: record("record-abandon"),
            head: head("main"),
            expected_head_revision: HeadRevision::new(1),
        })
        .unwrap_or_else(|error| panic!("abandon main: {error:?}"));
    let retired_state = journal.clone();
    let retired = JournalRecord::CreateHead {
        sequence: JournalSequence::new(3),
        record_id: record("record-retired"),
        head: head("main"),
        at: Some(root_id),
    };
    assert_eq!(
        journal.apply(retired),
        Err(JournalError::UnavailableHeadName(head("main")))
    );
    assert_eq!(journal, retired_state);
}

/// JRN-3: every nested model item keeps its tagged wire form inside an append.
#[test]
fn jrn_3_every_model_item_variant_round_trips_inside_an_append() {
    let call_id = id("call-1", ToolCallId::new);
    let outcomes = [
        ToolOutcome::Succeeded {
            output: "ok".to_owned(),
        },
        ToolOutcome::Failed {
            message: "failed".to_owned(),
        },
        ToolOutcome::AdmissionRefused {
            reason: AdmissionRefusal::StalePrecondition,
        },
        ToolOutcome::Forbidden,
        ToolOutcome::Denied,
        ToolOutcome::Cancelled {
            reason: ToolCancellationReason::Interrupted,
        },
    ];
    let agent_id = id("agent-a", AgentId::new);
    let item_id = id("tool-item", TranscriptItemId::new);
    let mut payloads = [
        (TranscriptRole::User, "user"),
        (TranscriptRole::Assistant, "assistant"),
        (TranscriptRole::Reasoning, "reasoning"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, text))| JournalEntryPayload::Message {
        agent_id: agent_id.clone(),
        item_id: id(&format!("message-{index}"), TranscriptItemId::new),
        role,
        text: text.to_owned(),
    })
    .collect::<Vec<_>>();
    payloads.push(JournalEntryPayload::ToolCallRequested {
        agent_id: agent_id.clone(),
        item_id,
        call: ToolCall {
            call_id: call_id.clone(),
            name: "read_file".to_owned(),
            arguments: r#"{"path":"README.md"}"#.to_owned(),
        },
        presentation: ToolPresentation::default(),
    });
    payloads.extend(
        outcomes
            .into_iter()
            .map(|outcome| JournalEntryPayload::ToolCallChanged {
                agent_id: agent_id.clone(),
                call_id: call_id.clone(),
                item_revision: 1,
                status: outcome.status(),
                presentation: ToolPresentation::default(),
                outcome: Some(outcome),
            }),
    );

    for (index, payload) in payloads.into_iter().enumerate() {
        let record = append(
            1,
            &format!("record-{index}"),
            "main",
            0,
            SessionEntry {
                id: id(&format!("entry-{index}"), SessionEntryId::new),
                parent_id: None,
                payload,
            },
        );
        let json = serde_json::to_string(&record)
            .unwrap_or_else(|error| panic!("encode nested item: {error}"));
        let decoded = serde_json::from_str::<JournalRecord>(&json)
            .unwrap_or_else(|error| panic!("decode nested item: {error}"));
        assert_eq!(decoded, record);
    }
}
