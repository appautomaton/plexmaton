use plexmaton_core::{
    AgentStatus, SessionEvent, SessionId, TokenCounts, TokenUsage, TranscriptItemId, TurnId,
};

use super::super::{JournalEntryPayload, SessionJournal};
use super::tests::{agent, append, head, id, message};
use crate::test_support::{output_with_replay, reasoning_block, replay, step};
use crate::{AssistantBlock, ContextAtomValue};

/// JRN-5: reasoning, usage and notices keep their separate consumer visibility.
#[test]
fn jrn_5_hidden_replay_and_visible_diagnostics_project_to_their_exact_consumers() {
    let agent_id = agent();
    let replay = replay("encrypted-secret");
    let usage = TokenUsage::Complete(TokenCounts {
        input: 10,
        cached_input: Some(6),
        cache_write_input: None,
        output: 3,
        reasoning_output: Some(1),
        total: 13,
    });
    let mut journal = SessionJournal::new(id("session-a", SessionId::new));
    append(
        &mut journal,
        1,
        JournalEntryPayload::AgentCreated {
            agent_id: agent_id.clone(),
            label: "Plexmaton".to_owned(),
            status: AgentStatus::Idle,
        },
    );
    append(
        &mut journal,
        2,
        message(plexmaton_core::TranscriptRole::User, 1, "inspect"),
    );
    append(
        &mut journal,
        3,
        JournalEntryPayload::AssistantOutput {
            agent_id: agent_id.clone(),
            step_id: step("turn-1", 1),
            output: output_with_replay(
                vec![reasoning_block("reasoning", "bounded reasoning")],
                [(0, replay.clone())],
            ),
        },
    );
    append(
        &mut journal,
        4,
        JournalEntryPayload::TurnUsageUpdated {
            agent_id: agent_id.clone(),
            turn_id: id("turn-1", TurnId::new),
            usage: usage.clone(),
        },
    );
    append(
        &mut journal,
        5,
        JournalEntryPayload::RuntimeWarning {
            agent_id,
            item_id: id("warning", TranscriptItemId::new),
            message: "recovered valid prefix".to_owned(),
        },
    );

    let projection = journal
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project journal: {error:?}"));
    assert_eq!(projection.request().atoms.len(), 2);
    let ContextAtomValue::Assistant(output) = projection.request().atoms[1].value() else {
        panic!("reasoning output projects as one assistant atom")
    };
    assert!(matches!(
        output.blocks(),
        [AssistantBlock::Reasoning { text, .. }] if text == "bounded reasoning"
    ));
    assert_eq!(
        output
            .replay()
            .and_then(|replay| replay.attachments().first())
            .map(|attachment| attachment.payload()),
        Some(replay.payload())
    );
    assert!(projection.events().iter().any(|event| matches!(
        &event.event,
        SessionEvent::TurnUsageUpdated {
            usage: actual, ..
        } if actual == &usage
    )));
    assert!(projection.events().iter().any(|event| matches!(
        &event.event,
        SessionEvent::RuntimeWarning { message, .. } if message == "recovered valid prefix"
    )));
    assert!(!format!("{projection:?}").contains("encrypted-secret"));
}
