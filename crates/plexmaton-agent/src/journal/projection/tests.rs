use plexmaton_core::{
    AgentId, AgentStatus, HeadName, JournalRecordId, SessionEntryId, SessionEvent, SessionId,
    ToolCallId, ToolCallStatus, ToolPresentation, TranscriptItemId, TranscriptRole, TurnId,
};

use super::super::{
    HeadRevision, JournalEntryPayload, JournalRecord, SessionEntry, SessionJournal,
};
use crate::test_support::{call_block, output, step, text_block};
use crate::{AssistantBlock, ContextAtomValue, ToolCall, ToolOutcome, UnixMillis};

pub(super) fn id<T>(
    value: &str,
    build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>,
) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

pub(super) fn head(value: &str) -> HeadName {
    id(value, HeadName::new)
}

pub(super) fn agent() -> AgentId {
    id("agent-a", AgentId::new)
}

pub(super) fn call(value: &str, output: &str) -> (ToolCall, ToolOutcome) {
    (
        ToolCall {
            call_id: id(value, ToolCallId::new),
            name: "read_file".to_owned(),
            arguments: format!(r#"{{"path":"{value}.txt"}}"#),
        },
        ToolOutcome::Succeeded {
            output: output.to_owned(),
        },
    )
}

pub(super) fn append(journal: &mut SessionJournal, ordinal: u64, payload: JournalEntryPayload) {
    let main = head("main");
    let parent_id = journal
        .head_target(&main)
        .unwrap_or_else(|error| panic!("main target: {error:?}"))
        .cloned();
    let revision = journal
        .head_revision(&main)
        .unwrap_or_else(|error| panic!("main revision: {error:?}"));
    journal
        .apply(JournalRecord::AppendEntry {
            sequence: journal.next_sequence(),
            record_id: id(&format!("record-{ordinal}"), JournalRecordId::new),
            head: main,
            expected_head_revision: revision,
            entry: Box::new(SessionEntry {
                id: id(&format!("entry-{ordinal}"), SessionEntryId::new),
                parent_id,
                payload,
            }),
        })
        .unwrap_or_else(|error| panic!("append fixture: {error:?}"));
}

pub(super) fn message(role: TranscriptRole, ordinal: u64, text: &str) -> JournalEntryPayload {
    if role == TranscriptRole::User {
        return JournalEntryPayload::TurnStarted {
            agent_id: agent(),
            item_id: id(&format!("item-{ordinal}"), TranscriptItemId::new),
            turn_id: id(&format!("turn-{ordinal}"), TurnId::new),
            text: text.to_owned(),
            accepted_at: UnixMillis::EPOCH,
            opened_at: UnixMillis::EPOCH,
        };
    }
    assert_eq!(role, TranscriptRole::System, "non-user message fixture");
    JournalEntryPayload::RuntimeWarning {
        agent_id: agent(),
        item_id: id(&format!("item-{ordinal}"), TranscriptItemId::new),
        message: text.to_owned(),
    }
}

pub(super) fn assistant_calls(
    turn: u64,
    step_index: u16,
    blocks: Vec<AssistantBlock>,
) -> JournalEntryPayload {
    JournalEntryPayload::AssistantOutput {
        agent_id: agent(),
        step_id: step(&format!("turn-{turn}"), step_index),
        output: output(blocks),
    }
}

fn finish_turn(journal: &mut SessionJournal, ordinal: u64) {
    let main = head("main");
    let boundary = journal
        .head_target(&main)
        .unwrap_or_else(|error| panic!("main target: {error:?}"))
        .cloned()
        .unwrap_or_else(|| panic!("turn has a semantic boundary"));
    let revision = journal
        .head_revision(&main)
        .unwrap_or_else(|error| panic!("main revision: {error:?}"));
    journal
        .apply(JournalRecord::TurnFinished {
            sequence: journal.next_sequence(),
            record_id: id(&format!("turn-finish-{ordinal}"), JournalRecordId::new),
            head: main,
            expected_head_revision: revision,
            fact: crate::TurnFinished {
                agent_id: agent(),
                turn_id: id(&format!("turn-{ordinal}"), TurnId::new),
                semantic_boundary: boundary,
                outcome: crate::TurnOutcome::Completed,
                at: crate::TurnFinishedAt::Observed {
                    completed_at: UnixMillis::EPOCH,
                },
            },
        })
        .unwrap_or_else(|error| panic!("finish fixture turn: {error:?}"));
}

pub(super) fn announce(journal: &mut SessionJournal) {
    append(
        journal,
        0,
        JournalEntryPayload::AgentCreated {
            agent_id: agent(),
            label: "Plexmaton".to_owned(),
            status: AgentStatus::Idle,
        },
    );
}

/// JRN-5: one path supplies both consumers and preserves model call order.
#[test]
fn jrn_5_one_path_projects_model_order_and_visible_lifecycle() {
    let mut journal = SessionJournal::new(id("session-a", SessionId::new));
    append(
        &mut journal,
        1,
        JournalEntryPayload::AgentCreated {
            agent_id: agent(),
            label: "Plexmaton".to_owned(),
            status: AgentStatus::Idle,
        },
    );
    append(&mut journal, 2, message(TranscriptRole::User, 1, "inspect"));
    let (first, first_outcome) = call("call-a", "first");
    let (second, second_outcome) = call("call-b", "second");
    append(
        &mut journal,
        3,
        assistant_calls(
            1,
            1,
            vec![
                text_block("item-2", "checking"),
                call_block("tool-a", first.clone()),
                call_block("tool-b", second.clone()),
            ],
        ),
    );
    for (ordinal, call_id) in [(4, first.call_id.clone()), (5, second.call_id.clone())] {
        append(
            &mut journal,
            ordinal,
            JournalEntryPayload::ToolCallRequested {
                agent_id: agent(),
                call_id,
                presentation: ToolPresentation::default(),
            },
        );
    }
    for (ordinal, call_id) in [(6, first.call_id.clone()), (7, second.call_id.clone())] {
        append(
            &mut journal,
            ordinal,
            JournalEntryPayload::ToolCallChanged {
                agent_id: agent(),
                call_id,
                item_revision: 1,
                status: ToolCallStatus::Running,
                presentation: ToolPresentation::default(),
                outcome: None,
            },
        );
    }
    for (ordinal, call_id, outcome) in [
        (8, second.call_id.clone(), second_outcome.clone()),
        (9, first.call_id.clone(), first_outcome.clone()),
    ] {
        append(
            &mut journal,
            ordinal,
            JournalEntryPayload::ToolCallChanged {
                agent_id: agent(),
                call_id,
                item_revision: 2,
                status: ToolCallStatus::Succeeded,
                presentation: ToolPresentation::default(),
                outcome: Some(outcome),
            },
        );
    }

    let projection = journal
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project journal: {error:?}"));
    assert_eq!(projection.request().atoms.len(), 2);
    let ContextAtomValue::ToolBatch(batch) = projection.request().atoms[1].value() else {
        panic!("complete calls and results project as one tool batch")
    };
    assert_eq!(
        batch
            .assistant()
            .tool_calls()
            .map(|call| call.call_id.as_str())
            .collect::<Vec<_>>(),
        ["call-a", "call-b"]
    );
    assert!(matches!(
        batch.assistant().blocks().first(),
        Some(AssistantBlock::Text { text, .. }) if text == "checking"
    ));
    assert_eq!(
        batch
            .results()
            .iter()
            .map(|result| result.call_id().as_str())
            .collect::<Vec<_>>(),
        ["call-a", "call-b"]
    );
    assert_eq!(batch.results()[0].outcome(), &first_outcome);
    assert_eq!(batch.results()[1].outcome(), &second_outcome);
    assert!(projection.recovery().is_none());
    assert!(
        projection
            .events()
            .iter()
            .enumerate()
            .all(|(index, event)| {
                event.sequence.get()
                    == u64::try_from(index + 1)
                        .unwrap_or_else(|error| panic!("event index exceeds u64: {error}"))
            })
    );
    let completions: Vec<_> = projection
        .events()
        .iter()
        .filter_map(|event| match &event.event {
            SessionEvent::ToolCallChanged {
                call_id,
                status: ToolCallStatus::Succeeded,
                ..
            } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(completions, ["call-b", "call-a"]);
}

/// JRN-5: an incomplete final batch is visible but cannot enter provider input unmatched.
#[test]
fn jrn_5_incomplete_tool_batch_is_explicit_and_absent_from_the_request() {
    let mut journal = SessionJournal::new(id("session-a", SessionId::new));
    announce(&mut journal);
    append(&mut journal, 1, message(TranscriptRole::User, 1, "inspect"));
    let (first, _) = call("call-a", "first");
    let (second, second_outcome) = call("call-b", "second");
    append(
        &mut journal,
        2,
        assistant_calls(
            1,
            1,
            vec![
                call_block("tool-a", first.clone()),
                call_block("tool-b", second.clone()),
            ],
        ),
    );
    for (ordinal, call_id) in [(3, first.call_id.clone()), (4, second.call_id.clone())] {
        append(
            &mut journal,
            ordinal,
            JournalEntryPayload::ToolCallRequested {
                agent_id: agent(),
                call_id,
                presentation: ToolPresentation::default(),
            },
        );
    }
    append(
        &mut journal,
        5,
        JournalEntryPayload::ToolCallChanged {
            agent_id: agent(),
            call_id: second.call_id.clone(),
            item_revision: 1,
            status: ToolCallStatus::Running,
            presentation: ToolPresentation::default(),
            outcome: None,
        },
    );
    append(
        &mut journal,
        6,
        JournalEntryPayload::ToolCallChanged {
            agent_id: agent(),
            call_id: second.call_id,
            item_revision: 2,
            status: ToolCallStatus::Succeeded,
            presentation: ToolPresentation::default(),
            outcome: Some(second_outcome),
        },
    );

    let projection = journal
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project journal: {error:?}"));
    assert_eq!(projection.request().atoms.len(), 1);
    let recovery = projection
        .recovery()
        .unwrap_or_else(|| panic!("missing recovery projection"));
    assert_eq!(
        recovery
            .omitted_batch_calls()
            .iter()
            .map(ToolCallId::as_str)
            .collect::<Vec<_>>(),
        ["call-a", "call-b"]
    );
    assert!(projection.events().iter().any(|event| matches!(
        event.event,
        SessionEvent::ToolCallChanged {
            status: ToolCallStatus::Succeeded,
            ..
        }
    )));
}

/// JRN-5: two named heads cannot leak model or screen facts into one another.
#[test]
fn jrn_5_named_heads_project_only_their_selected_ancestry() {
    let mut journal = SessionJournal::new(id("session-a", SessionId::new));
    announce(&mut journal);
    append(&mut journal, 1, message(TranscriptRole::User, 1, "root"));
    finish_turn(&mut journal, 1);
    let root = journal
        .head_target(&head("main"))
        .unwrap_or_else(|error| panic!("main target: {error:?}"))
        .cloned();
    for name in ["left", "right"] {
        let sequence = journal.next_sequence();
        journal
            .apply(JournalRecord::CreateHead {
                sequence,
                record_id: id(&format!("record-{name}"), JournalRecordId::new),
                head: head(name),
                at: root.clone(),
            })
            .unwrap_or_else(|error| panic!("create {name}: {error:?}"));
    }
    for (name, ordinal, text) in [("left", 4, "left only"), ("right", 5, "right only")] {
        let selected = head(name);
        journal
            .apply(JournalRecord::AppendEntry {
                sequence: journal.next_sequence(),
                record_id: id(&format!("record-{ordinal}"), JournalRecordId::new),
                head: selected.clone(),
                expected_head_revision: HeadRevision::new(0),
                entry: Box::new(SessionEntry {
                    id: id(&format!("entry-{ordinal}"), SessionEntryId::new),
                    parent_id: root.clone(),
                    payload: message(TranscriptRole::User, ordinal, text),
                }),
            })
            .unwrap_or_else(|error| panic!("append {name}: {error:?}"));
    }

    let left = journal
        .project(&head("left"))
        .unwrap_or_else(|error| panic!("project left: {error:?}"));
    let right = journal
        .project(&head("right"))
        .unwrap_or_else(|error| panic!("project right: {error:?}"));
    assert_eq!(
        left.request().atoms.last().map(|atom| atom.value()),
        Some(&ContextAtomValue::User {
            text: "left only".to_owned(),
        })
    );
    assert_eq!(
        right.request().atoms.last().map(|atom| atom.value()),
        Some(&ContextAtomValue::User {
            text: "right only".to_owned(),
        })
    );
    let replayed_left = journal
        .project(&head("left"))
        .unwrap_or_else(|error| panic!("reproject left: {error:?}"));
    assert_eq!(left, replayed_left);
}
