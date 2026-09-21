use super::tests::{agent, head};
use crate::{Agent, Effect, Input, ModelEvent, ModelOutputPosition, StopReason};

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
        event: ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 0),
            delta: "h".to_owned(),
        },
    });
    let _second_delta = live.handle(Input::Streamed {
        step_id: step_id.clone(),
        event: ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 0),
            delta: "i".to_owned(),
        },
    });
    let _stopped = live.handle(Input::Streamed {
        step_id,
        event: ModelEvent::Stopped(StopReason::EndOfTurn),
    });

    let projection = live
        .journal()
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project live journal: {error:?}"));

    assert_eq!(projection.request().atoms, live.record());
}

/// ENT-3: a call the provider ran is one event in its terminal state, and the journal's replay says
/// the same event the live step said, so a reopened conversation shows the same row.
#[test]
fn ent_3_a_server_tool_call_is_the_same_event_live_and_on_replay() {
    use plexmaton_core::{
        ConversationEvent, ServerTool, ServerToolAction, ServerToolCall, ServerToolStatus,
    };
    let agent_id = agent();
    let mut live = Agent::new(agent_id.clone());
    let _announcement = live.announce("Plexmaton");
    let submitted = live.handle(Input::Submitted {
        text: "latest rust?".to_owned(),
    });
    let step_id = match submitted.effects.as_slice() {
        [Effect::CallModel(call)] => call.step_id.clone(),
        other => panic!("expected one model call, got {other:?}"),
    };
    let call = ServerToolCall {
        tool: ServerTool::WebSearch,
        action: ServerToolAction::Search {
            queries: vec!["latest stable Rust release".to_owned()],
        },
        status: ServerToolStatus::Failed,
    };
    let searched = live.handle(Input::Streamed {
        step_id: step_id.clone(),
        event: ModelEvent::ServerToolCall {
            position: ModelOutputPosition::new(0, 0),
            call: call.clone(),
        },
    });
    let _answer = live.handle(Input::Streamed {
        step_id: step_id.clone(),
        event: ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 1),
            delta: "1.98.1".to_owned(),
        },
    });
    let _stopped = live.handle(Input::Streamed {
        step_id,
        event: ModelEvent::Stopped(StopReason::EndOfTurn),
    });
    let live_events: Vec<_> = searched
        .events
        .iter()
        .map(|envelope| envelope.event.clone())
        .filter(|event| matches!(event, ConversationEvent::ServerToolCalled { .. }))
        .collect();
    assert!(
        matches!(
            live_events.as_slice(),
            [ConversationEvent::ServerToolCalled { agent_id: live_agent, call: live_call, .. }]
                if live_agent == &agent_id && live_call == &call
        ),
        "{live_events:?}"
    );

    let projection = live
        .journal()
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project live journal: {error:?}"));
    let replayed: Vec<_> = projection
        .events()
        .iter()
        .map(|envelope| envelope.event.clone())
        .filter(|event| matches!(event, ConversationEvent::ServerToolCalled { .. }))
        .collect();
    assert_eq!(replayed, live_events);
}
