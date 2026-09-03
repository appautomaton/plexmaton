use super::*;

/// LIVE-3 and LIVE-5: both an explicit transport failure and a task that returns without a
/// terminal signal leave no task alive and produce an unavailable usage state.
#[tokio::test]
async fn deterministic_failure_paths_leave_no_provider_task_alive() {
    for script in [
        Script::Fail(ModelError::Transport {
            message: "offline".to_owned(),
        }),
        Script::EndWithoutTerminal,
    ] {
        let mut runtime = runtime(FakeDriver::new([script]));
        let _announced = runtime.try_next_event();
        runtime
            .submit(
                agent_id(),
                Input::Submitted {
                    text: "begin".to_owned(),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("submission: {error}"));
        let events = finish_active(&mut runtime).await;

        assert!(!runtime.has_active_model());
        assert!(events.iter().any(|envelope| matches!(
            envelope.event,
            SessionEvent::TurnUsageUpdated {
                usage: TokenUsage::Unavailable,
                ..
            }
        )));
        assert!(
            events
                .iter()
                .any(|envelope| matches!(envelope.event, SessionEvent::RuntimeWarning { .. }))
        );
    }
}

/// LIVE-3: cancelling `next_event` while a terminal signal waits for its task to join leaves the
/// exact handle owned, so a later interrupt can cancel and join it rather than detaching it.
#[tokio::test]
async fn a_cancelled_terminal_join_remains_owned_until_interrupt_joins_it() {
    let ready = Arc::new(Notify::new());
    let finished = Arc::new(AtomicBool::new(false));
    let mut runtime = runtime(FakeDriver::new([Script::TerminalThenWaitForCancellation {
        ready: Arc::clone(&ready),
        finished: Arc::clone(&finished),
    }]));
    let _announced = runtime.try_next_event();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "begin".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submission: {error}"));
    let mut events = Vec::new();
    take_ready(&mut runtime, &mut events);
    loop {
        match runtime.next_event().now_or_never() {
            Some(Ok(Some(event))) => events.push(event),
            None => break,
            other => panic!("terminal cleanup should be the only pending work: {other:?}"),
        }
    }
    assert!(ready.notified().now_or_never().is_some());
    assert!(
        runtime.has_active_model(),
        "cancelling the wait detached its task"
    );
    assert!(!finished.load(Ordering::SeqCst));

    let report = runtime
        .submit(agent_id(), Input::Interrupted)
        .await
        .unwrap_or_else(|error| panic!("interrupt: {error}"));

    assert!(!runtime.has_active_model());
    assert!(
        finished.load(Ordering::SeqCst),
        "interrupt returned before join"
    );
    assert_eq!(report.undelivered_model.len(), 1);
}
