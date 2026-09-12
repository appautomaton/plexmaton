//! What the runtime says is still waiting to be sent.

use plexmaton_agent::{Input, ModelEvent, StopReason};

use super::{FakeDriver, Script, agent_id, complete_usage, finish_active, runtime, text_delta};
use crate::QueuedBoundary;

/// IQU-1: owned skill completion wakes the projection even when the model emits no further data.
#[tokio::test]
async fn completed_skill_input_wakes_the_waiting_projection_without_a_model_delta() {
    use super::{Script, tools::TestWorkspace};
    use crate::{LiveRuntime, RuntimeUpdate};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;
    use tokio::sync::Notify;

    let files = TestWorkspace::new("waiting-skill-wake");
    let directory = files.0.join(".agents/skills/review");
    std::fs::create_dir_all(&directory).expect("skill directory");
    std::fs::write(
        directory.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nKeep the queue exact.\n",
    )
    .expect("skill fixture");
    let tools = files
        .catalog()
        .with_skill_roots(
            &files.0,
            &files.0,
            &plexmaton_file_tools::FileCancellation::new(),
        )
        .expect("skill catalog");
    let started = Arc::new(Notify::new());
    let finished = Arc::new(AtomicBool::new(false));
    let driver = FakeDriver::new([Script::WaitForCancellation {
        started: started.clone(),
        finished: finished.clone(),
    }]);
    let mut runtime =
        LiveRuntime::with_driver(agent_id(), "Plexmaton".to_owned(), driver.clone(), tools)
            .expect("runtime");
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "first".to_owned(),
            },
        )
        .await
        .expect("first turn");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let RuntimeUpdate::Event(event) = runtime.next_update().await.expect("model update")
                && matches!(
                    event.event,
                    plexmaton_core::ConversationEvent::TranscriptDelta { .. }
                )
            {
                break;
            }
        }
    })
    .await
    .expect("model emitted its only delta");
    runtime
        .submit_skill(
            agent_id(),
            Input::Submitted {
                text: "$review inspect the queue".to_owned(),
            },
            "review".to_owned(),
        )
        .await
        .expect("prepare explicit skill");

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match runtime.next_update().await.expect("owned update") {
                RuntimeUpdate::Report(report) if report.queued_input_changed => break,
                RuntimeUpdate::Finished => panic!("the provider is still waiting"),
                RuntimeUpdate::Report(_) | RuntimeUpdate::Event(_) => {}
            }
        }
    })
    .await
    .expect("skill completion must wake the projection without another event");
    assert_eq!(
        runtime
            .queued_input()
            .map(|input| (input.text, input.boundary))
            .collect::<Vec<_>>(),
        [("$review inspect the queue", QueuedBoundary::Turn)]
    );
    assert!(!finished.load(Ordering::SeqCst));
    assert_eq!(driver.calls().await.len(), 1);
    let report = runtime
        .withdraw_queued(&agent_id())
        .expect("withdraw prepared skill");
    assert_eq!(report.undelivered[0].text, "$review inspect the queue");
    assert_eq!(report.undelivered[0].skill.as_deref(), Some("review"));
    runtime
        .shutdown()
        .await
        .expect("join the still-running model");
    assert!(finished.load(Ordering::SeqCst));
}

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

/// IQU-1/IQU-4: input this runtime is still holding is reported, and taken back, as its own place.
///
/// A compaction the user asked for owns the agent until its checkpoint, so everything submitted
/// meanwhile waits here rather than in a queue the agent has (CPL-9). Read only the agent's two
/// queues and this fails: the band reports nothing while two messages wait, and `Alt-↑` has
/// nothing to take back.
#[tokio::test]
async fn input_held_by_an_owned_operation_is_reported_and_taken_back_newest_first() {
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let driver = super::compaction::CompactionDriver::new(
        [Script::Events(vec![
            text_delta(&super::compaction::large_answer()),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ])],
        [super::compaction::SummaryScript::WaitForCancellation(
            std::sync::Arc::clone(&cancelled),
        )],
    );
    let mut runtime = runtime(driver.clone());
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "seed".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("seed: {error}"));
    let _events = finish_active(&mut runtime).await;
    driver.enable();
    match runtime
        .request_compaction(agent_id())
        .await
        .unwrap_or_else(|error| panic!("request compaction: {error}"))
    {
        crate::CompactionRequest::Started { .. } => {}
        crate::CompactionRequest::Refused(refusal) => panic!("idle request refused: {refusal:?}"),
    }

    for text in ["older waiting", "newest waiting"] {
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
    assert_eq!(
        runtime
            .queued_input()
            .map(|queued| (queued.text.to_owned(), queued.boundary))
            .collect::<Vec<_>>(),
        [
            ("older waiting".to_owned(), QueuedBoundary::Admission),
            ("newest waiting".to_owned(), QueuedBoundary::Admission),
        ],
        "held input is reported in arrival order, as waiting on the operation"
    );

    let report = runtime
        .withdraw_queued(&agent_id())
        .unwrap_or_else(|error| panic!("withdraw: {error}"));
    assert_eq!(
        report
            .undelivered
            .iter()
            .map(|input| (input.text.clone(), input.reason))
            .collect::<Vec<_>>(),
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
        "the operation still holds the one the user did not name"
    );

    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));
    assert!(
        cancelled.load(std::sync::atomic::Ordering::SeqCst),
        "the compaction that was holding the input is cancelled and joined"
    );
}
