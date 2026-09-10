//! What the runtime says is still waiting to be sent.

use plexmaton_agent::{Input, ModelEvent, StopReason};

use super::{FakeDriver, Script, agent_id, complete_usage, finish_active, runtime, text_delta};
use crate::QueuedBoundary;

/// IQU-1: a message typed while the model answers is waiting, and says which boundary claims it.
///
/// Drop the `queued_for_next_turn` source from the projection and this fails: the runtime reports
/// nothing waiting while the agent is holding text the user has already pressed `Enter` on, which
/// is exactly the invisible queue the band exists to end.
#[tokio::test]
async fn a_message_typed_mid_turn_is_reported_until_its_boundary_claims_it() {
    let driver = FakeDriver::new([
        Script::Events(vec![
            text_delta("first answer"),
            complete_usage(10, 3),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
        Script::Events(vec![
            text_delta("second answer"),
            complete_usage(10, 3),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
    ]);
    let mut runtime = runtime(driver);
    let _announced = runtime.try_next_event();

    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "first".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("open the first turn: {error}"));
    assert!(runtime.has_active_model(), "the first turn is running");
    assert_eq!(runtime.queued_input().count(), 0, "nothing waits yet");

    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "and also this".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("queue the second message: {error}"));

    let waiting: Vec<_> = runtime
        .queued_input()
        .map(|queued| (queued.text.to_owned(), queued.boundary))
        .collect();
    assert_eq!(
        waiting,
        [("and also this".to_owned(), QueuedBoundary::Turn)],
        "the exact text, and the boundary that will say it"
    );

    finish_active(&mut runtime).await;
    assert_eq!(
        runtime.queued_input().count(),
        0,
        "a claimed message is no longer waiting"
    );
}

/// IQU-4: taking a message back returns the exact text and leaves the queue without it.
///
/// Withdraw from the front instead of the back and this fails: the user pressed `Alt-↑` to undo
/// the `Enter` they just pressed, and the oldest waiting message is not the one they meant.
#[tokio::test]
async fn the_newest_waiting_message_comes_back_with_its_exact_text() {
    let driver = FakeDriver::new([Script::Events(vec![
        text_delta("first answer"),
        complete_usage(10, 3),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let mut runtime = runtime(driver);
    let _announced = runtime.try_next_event();

    for text in ["first", "older waiting", "newest waiting"] {
        runtime
            .submit(
                agent_id(),
                Input::Submitted {
                    text: text.to_owned(),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("submit {text}: {error}"));
    }
    assert_eq!(runtime.queued_input().count(), 2);

    let report = runtime
        .withdraw_queued(&agent_id())
        .unwrap_or_else(|error| panic!("withdraw: {error}"));
    let returned: Vec<_> = report
        .undelivered
        .iter()
        .map(|input| (input.text.clone(), input.reason))
        .collect();
    assert_eq!(
        returned,
        [(
            "newest waiting".to_owned(),
            plexmaton_agent::UndeliveredReason::Withdrawn
        )]
    );
    assert_eq!(
        runtime
            .queued_input()
            .map(|queued| queued.text.to_owned())
            .collect::<Vec<_>>(),
        ["older waiting".to_owned()],
        "only the one the user named left the queue"
    );

    let second = runtime
        .withdraw_queued(&agent_id())
        .unwrap_or_else(|error| panic!("withdraw again: {error}"));
    assert_eq!(second.undelivered.len(), 1);
    assert_eq!(runtime.queued_input().count(), 0);

    let empty = runtime
        .withdraw_queued(&agent_id())
        .unwrap_or_else(|error| panic!("withdraw from an empty queue: {error}"));
    assert!(
        empty.undelivered.is_empty(),
        "nothing waiting is nothing to take back, not an error"
    );
}

/// IQU-4: a withdrawal is not a turn boundary — it starts nothing and cancels nothing.
#[tokio::test]
async fn taking_a_message_back_leaves_the_running_turn_alone() {
    let driver = FakeDriver::new([Script::Events(vec![
        text_delta("first answer"),
        complete_usage(10, 3),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let mut runtime = runtime(driver);
    let _announced = runtime.try_next_event();

    for text in ["first", "second"] {
        runtime
            .submit(
                agent_id(),
                Input::Submitted {
                    text: text.to_owned(),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("submit {text}: {error}"));
    }
    let active = runtime.has_active_model();
    runtime
        .withdraw_queued(&agent_id())
        .unwrap_or_else(|error| panic!("withdraw: {error}"));

    assert!(active && runtime.has_active_model(), "the turn still runs");
    let events = finish_active(&mut runtime).await;
    assert!(
        events.iter().any(|event| matches!(
            &event.event,
            plexmaton_core::ConversationEvent::TranscriptDelta { text, .. } if text == "first answer"
        )),
        "the answer the user was waiting for still arrives"
    );
}
