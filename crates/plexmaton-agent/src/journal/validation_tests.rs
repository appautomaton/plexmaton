use plexmaton_core::{
    AgentId, AgentStatus, ConversationEntryId, ConversationId, HeadName, JournalRecordId,
    ToolCallId, ToolPresentation, TranscriptItemId,
};

use super::{
    ConversationEntry, ConversationJournal, HeadRevision, JournalEntryPayload, JournalError,
    JournalRecord, JournalSequence,
};
use crate::test_support::{call_block, output, reasoning_block, step, text_block};
use crate::{AdmissionRefusal, ToolCall, ToolCancellationReason, ToolOutcome, UnixMillis};

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn head(value: &str) -> HeadName {
    id(value, HeadName::new)
}

fn record(value: &str) -> JournalRecordId {
    id(value, JournalRecordId::new)
}

fn entry(value: &str, parent_id: Option<ConversationEntryId>) -> ConversationEntry {
    ConversationEntry {
        id: id(value, ConversationEntryId::new),
        parent_id,
        payload: JournalEntryPayload::RuntimeWarning {
            agent_id: id("agent-a", AgentId::new),
            item_id: id(&format!("item-{value}"), TranscriptItemId::new),
            message: value.to_owned(),
        },
    }
}

fn append(
    sequence: u64,
    record_id: &str,
    head_name: &str,
    revision: u64,
    entry: ConversationEntry,
) -> JournalRecord {
    JournalRecord::AppendEntry {
        sequence: JournalSequence::new(sequence),
        record_id: record(record_id),
        head: head(head_name),
        expected_head_revision: HeadRevision::new(revision),
        entry: Box::new(entry),
    }
}

fn rooted() -> (ConversationJournal, ConversationEntryId) {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    let root = entry("entry-1", None);
    let root_id = root.id.clone();
    journal
        .apply(append(1, "record-1", "main", 0, root))
        .unwrap_or_else(|error| panic!("append root: {error:?}"));
    (journal, root_id)
}

/// TIM-1: agent creation cannot bypass its typed lifecycle boundary.
#[test]
fn tim_1_invalid_initial_agent_status_changes_nothing() {
    let mut journal = ConversationJournal::new(id("session-a", ConversationId::new));
    let agent_id = id("agent-a", AgentId::new);
    let running_creation = append(
        1,
        "running-agent",
        "main",
        0,
        ConversationEntry {
            id: id("running-agent-entry", ConversationEntryId::new),
            parent_id: None,
            payload: JournalEntryPayload::AgentCreated {
                agent_id: agent_id.clone(),
                label: "Agent A".to_owned(),
                status: AgentStatus::Running,
            },
        },
    );
    let empty = journal.clone();
    assert_eq!(
        journal.apply(running_creation),
        Err(JournalError::InvalidInitialAgentStatus(agent_id.clone()))
    );
    assert_eq!(journal, empty);
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
    let missing = id("missing", ConversationEntryId::new);
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

/// JRN-3: every nested context block keeps its tagged wire form inside an append.
#[test]
fn jrn_3_every_context_block_variant_round_trips_inside_an_append() {
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
    let call = ToolCall {
        call_id: call_id.clone(),
        name: "read_file".to_owned(),
        arguments: r#"{"path":"README.md"}"#.to_owned(),
    };
    let mut payloads = vec![JournalEntryPayload::TurnStarted {
        agent_id: agent_id.clone(),
        item_id: id("message-user", TranscriptItemId::new),
        turn_id: id("turn-user", plexmaton_core::TurnId::new),
        text: "user".to_owned(),
        accepted_at: UnixMillis::EPOCH,
        opened_at: UnixMillis::EPOCH,
    }];
    payloads.extend([
        JournalEntryPayload::AssistantOutput {
            agent_id: agent_id.clone(),
            step_id: step("turn-user", 1),
            output: output(vec![text_block("message-assistant", "assistant")]),
        },
        JournalEntryPayload::AssistantOutput {
            agent_id: agent_id.clone(),
            step_id: step("turn-user", 2),
            output: output(vec![reasoning_block("message-reasoning", "reasoning")]),
        },
        JournalEntryPayload::AssistantOutput {
            agent_id: agent_id.clone(),
            step_id: step("turn-user", 3),
            output: output(vec![call_block("tool-item", call.clone())]),
        },
    ]);
    payloads.push(JournalEntryPayload::ToolCallRequested {
        agent_id: agent_id.clone(),
        call_id: call_id.clone(),
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
            ConversationEntry {
                id: id(&format!("entry-{index}"), ConversationEntryId::new),
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
