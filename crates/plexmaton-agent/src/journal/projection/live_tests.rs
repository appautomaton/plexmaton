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
