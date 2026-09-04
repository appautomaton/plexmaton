use plexmaton_core::{AgentStatus, SessionId, TranscriptItemId, TranscriptRole};

use super::super::{JournalEntryPayload, SessionJournal};
use super::tests::{agent, append, head, id};
use crate::{Agent, Effect, Input, ModelEvent, StopReason};

/// JRN-5: provider chunking does not change the canonical model context.
#[test]
fn jrn_5_canonical_live_turn_and_journal_replay_have_equal_model_context() {
    let agent_id = agent();
    let mut live = Agent::new(agent_id.clone());
    let _announcement = live.announce("Plexmaton");
    let submitted = live.handle(Input::Submitted {
        text: "hello".to_owned(),
    });
    let step_id = match submitted.effects.as_slice() {
        [Effect::CallModel(call)] => call.step_id.clone(),
        other => panic!("expected one model call, got {other:?}"),
    };
    let _first_delta = live.handle(Input::Streamed {
        step_id: step_id.clone(),
        event: ModelEvent::TextDelta("h".to_owned()),
    });
    let _second_delta = live.handle(Input::Streamed {
        step_id: step_id.clone(),
        event: ModelEvent::TextDelta("i".to_owned()),
    });
    let _stopped = live.handle(Input::Streamed {
        step_id,
        event: ModelEvent::Stopped(StopReason::EndOfTurn),
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
        JournalEntryPayload::Message {
            agent_id: agent_id.clone(),
            item_id: id("agent-a-1", TranscriptItemId::new),
            role: TranscriptRole::User,
            text: "hello".to_owned(),
        },
    );
    append(
        &mut journal,
        3,
        JournalEntryPayload::AgentStatusChanged {
            agent_id: agent_id.clone(),
            status: AgentStatus::Running,
        },
    );
    append(
        &mut journal,
        4,
        JournalEntryPayload::Message {
            agent_id: agent_id.clone(),
            item_id: id("agent-a-2", TranscriptItemId::new),
            role: TranscriptRole::Assistant,
            text: "hi".to_owned(),
        },
    );
    append(
        &mut journal,
        5,
        JournalEntryPayload::AgentStatusChanged {
            agent_id,
            status: AgentStatus::Idle,
        },
    );
    let projection = journal
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project journal: {error:?}"));

    assert_eq!(projection.request().items, live.record());
}
