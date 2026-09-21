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

/// ENT-2: live, a call the provider ran appears running where the provider placed it and finishes
/// at the next revision; the journal holds only the finished call, so replay says the same finished
/// row at revision zero, and a reopened conversation shows what the live one ended with.
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
    let started = live.handle(Input::Streamed {
        step_id: step_id.clone(),
        event: ModelEvent::ServerToolStarted {
            position: ModelOutputPosition::new(0, 0),
            tool: ServerTool::WebSearch,
        },
    });
    assert!(
        matches!(
            started.events.as_slice(),
            [envelope] if matches!(
                &envelope.event,
                ConversationEvent::ServerToolStarted { tool: ServerTool::WebSearch, .. }
            )
        ),
        "{started:?}"
    );
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
    let [
        ConversationEvent::ServerToolCalled {
            agent_id: live_agent,
            item_id: live_item,
            item_revision: 1,
            call: live_call,
        },
    ] = live_events.as_slice()
    else {
        panic!("{live_events:?}");
    };
    assert_eq!((live_agent, live_call), (&agent_id, &call));

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
    assert!(
        matches!(
            replayed.as_slice(),
            [ConversationEvent::ServerToolCalled { agent_id, item_id, item_revision: 0, call: replayed_call }]
                if agent_id == live_agent && item_id == live_item && replayed_call == live_call
        ),
        "{replayed:?}"
    );
}

/// ENT-2/PRV-5: a call still running when the step ends did not finish. The row says so, and the
/// record keeps no block for it, because there is nothing to replay.
#[test]
fn a_call_still_running_when_the_step_ends_is_shown_failed_and_kept_out_of_the_record() {
    use plexmaton_core::{ConversationEvent, ServerTool, ServerToolAction, ServerToolStatus};
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
    let _started = live.handle(Input::Streamed {
        step_id: step_id.clone(),
        event: ModelEvent::ServerToolStarted {
            position: ModelOutputPosition::new(0, 0),
            tool: ServerTool::WebSearch,
        },
    });
    let _answer = live.handle(Input::Streamed {
        step_id: step_id.clone(),
        event: ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 1),
            delta: "1.98.1".to_owned(),
        },
    });
    let stopped = live.handle(Input::Streamed {
        step_id,
        event: ModelEvent::Stopped(StopReason::EndOfTurn),
    });
    assert!(
        stopped.events.iter().any(|envelope| matches!(
            &envelope.event,
            ConversationEvent::ServerToolCalled { item_revision: 1, call, .. }
                if call.status == ServerToolStatus::Failed
                    && call.action == ServerToolAction::Search { queries: Vec::new() }
        )),
        "{stopped:?}"
    );
    let projection = live
        .journal()
        .project(&head("main"))
        .unwrap_or_else(|error| panic!("project live journal: {error:?}"));
    assert!(
        projection.events().iter().all(|envelope| !matches!(
            envelope.event,
            ConversationEvent::ServerToolStarted { .. }
                | ConversationEvent::ServerToolCalled { .. }
        )),
        "the record holds no call that never finished"
    );
}
