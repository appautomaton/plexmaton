use super::*;

/// JRN-7: cancelling the waiter leaves the accepted transition owned by the runtime.
#[tokio::test]
async fn cancelled_submit_keeps_commit_owned_until_next_poll() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, driver).await;
    let _announcement = runtime.try_next_event();
    control.block_after(1);
    {
        let entered = control.gate.entered.notified();
        let submit = runtime.submit(agent_id(), submission());
        tokio::pin!(submit);
        tokio::select! {
            result = &mut submit => panic!("submit completed before append ack: {result:?}"),
            () = entered => {}
        }
    }
    control.gate.release();

    let event = runtime
        .next_event()
        .await
        .unwrap_or_else(|error| panic!("resume pending commit: {error}"));
    assert!(event.is_some());
    assert!(runtime.has_active_model());
}

/// JRN-7: a failed commit outlives its cancelled waiter and returns both owned submissions.
#[tokio::test]
async fn failed_cancelled_submit_surfaces_on_the_next_submit_without_losing_either_text() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    control.block_after(1);
    control.fail_after(1, false);
    {
        let entered = control.gate.entered.notified();
        let submit = runtime.submit(agent_id(), submission());
        tokio::pin!(submit);
        tokio::select! {
            result = &mut submit => panic!("submit completed before injected failure: {result:?}"),
            () = entered => {}
        }
    }
    control.gate.release();

    let report = runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "also keep this".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("resume cancelled submit: {error}"));

    assert_eq!(
        report
            .undelivered
            .iter()
            .map(|input| input.text.as_str())
            .collect::<Vec<_>>(),
        ["keep this exact draft", "also keep this"]
    );
    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::NotWritten)
    );
    assert!(driver.calls().await.is_empty());
}

/// JRN-7/LIVE-1: pending recovery never attributes a new input to the wrong runtime owner.
#[tokio::test]
async fn wrong_agent_is_rejected_before_a_pending_failure_report_is_attributed() {
    let (control, store) = StoreControl::pair();
    let mut runtime = runtime(store, FakeDriver::new([Script::EndWithoutTerminal])).await;
    let _announcement = runtime.try_next_event();
    control.block_after(1);
    control.fail_after(1, false);
    {
        let entered = control.gate.entered.notified();
        let submit = runtime.submit(agent_id(), submission());
        tokio::pin!(submit);
        tokio::select! {
            result = &mut submit => panic!("submit completed before injected failure: {result:?}"),
            () = entered => {}
        }
    }
    control.gate.release();

    let wrong = AgentId::new("agent-wrong").unwrap_or_else(|error| panic!("wrong id: {error}"));
    assert!(matches!(
        runtime
            .submit(
                wrong,
                Input::Submitted {
                    text: "belongs elsewhere".to_owned(),
                },
            )
            .await,
        Err(RuntimeError::WrongAgent { .. })
    ));
    let report = await_report(&mut runtime).await;
    assert_eq!(
        report
            .undelivered
            .iter()
            .map(|input| input.text.as_str())
            .collect::<Vec<_>>(),
        ["keep this exact draft"]
    );
}

/// JRN-7: an interrupt is retained together with missing usage before the first await.
#[tokio::test]
async fn cancelled_interrupt_during_usage_append_still_joins_the_model() {
    let (control, store) = StoreControl::pair();
    let started = Arc::new(Notify::new());
    let finished = Arc::new(AtomicBool::new(false));
    let driver = FakeDriver::new([Script::WaitForCancellation {
        started,
        finished: Arc::clone(&finished),
    }]);
    let mut runtime = runtime(store, driver).await;
    let _announcement = runtime.try_next_event();
    runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("open turn: {error}"));
    control.block_after(1);

    {
        let entered = control.gate.entered.notified();
        let interrupt = runtime.submit(agent_id(), Input::Interrupted);
        tokio::pin!(interrupt);
        tokio::select! {
            result = &mut interrupt => panic!("interrupt completed before usage ack: {result:?}"),
            () = entered => {}
        }
    }
    assert!(runtime.has_active_model());
    control.gate.release();
    let _update = runtime
        .next_update()
        .await
        .unwrap_or_else(|error| panic!("resume interrupt: {error}"));

    assert!(!runtime.has_active_model());
    assert!(finished.load(Ordering::SeqCst));
}

/// JRN-7: a cancelled model-end poll keeps the retained provider owner until usage commits.
#[tokio::test]
async fn cancelled_model_end_during_usage_append_keeps_the_active_owner() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, driver).await;
    while runtime.try_next_event().is_some() {}
    runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("open turn: {error}"));
    while runtime.try_next_event().is_some() {}
    control.block_after(1);

    {
        let entered = control.gate.entered.notified();
        let update = runtime.next_update();
        tokio::pin!(update);
        tokio::select! {
            result = &mut update => panic!("model end completed before usage ack: {result:?}"),
            () = entered => {}
        }
    }
    assert!(runtime.has_active_model());
    control.gate.release();
    while runtime.has_active_model() {
        let _update = tokio::time::timeout(Duration::from_secs(5), runtime.next_update())
            .await
            .unwrap_or_else(|_| panic!("retained model end did not resume"))
            .unwrap_or_else(|error| panic!("resume model end: {error}"));
    }
}

/// JRN-7: terminal output is installed in its active owner before missing usage can suspend.
#[tokio::test]
async fn cancelled_terminal_usage_append_keeps_the_terminal_until_interrupt() {
    let (control, store) = StoreControl::pair();
    let ready = Arc::new(Notify::new());
    let finished = Arc::new(AtomicBool::new(false));
    let driver = FakeDriver::new([Script::TerminalThenWaitForCancellation {
        ready,
        finished: Arc::clone(&finished),
    }]);
    let mut runtime = runtime(store, driver).await;
    while runtime.try_next_event().is_some() {}
    runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("open turn: {error}"));
    while runtime.try_next_event().is_some() {}
    control.block_after(1);

    {
        let entered = control.gate.entered.notified();
        let update = runtime.next_update();
        tokio::pin!(update);
        tokio::select! {
            result = &mut update => panic!("terminal completed before usage ack: {result:?}"),
            () = entered => {}
        }
    }
    assert!(
        runtime
            .active
            .as_ref()
            .is_some_and(|active| active.terminal.is_some())
    );
    control.gate.release();
    runtime
        .submit(agent_id(), Input::Interrupted)
        .await
        .unwrap_or_else(|error| panic!("resume with interrupt: {error}"));

    assert!(!runtime.has_active_model());
    assert!(finished.load(Ordering::SeqCst));
}

/// JRN-7: cancelled shutdown resumes its accepted append and joins every owner.
#[tokio::test]
async fn cancelled_shutdown_drains_every_accepted_record_before_writer_exit() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, driver).await;
    let _announcement = runtime.try_next_event();
    runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("open durable turn: {error}"));
    control.block_after(1);

    {
        let entered = control.gate.entered.notified();
        let shutdown = runtime.shutdown();
        tokio::pin!(shutdown);
        tokio::select! {
            result = &mut shutdown => panic!("shutdown completed before append ack: {result:?}"),
            () = entered => {}
        }
    }
    control.gate.release();
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("resume durable shutdown: {error}"));

    assert!(!runtime.has_active_work());
    assert_eq!(
        control
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_slice(),
        runtime.agent.journal().records(),
        "the joined writer and authoritative in-memory journal must end on the same prefix"
    );
}

/// JRN-7/LOOP-6: failed shutdown joins work and returns queued text in original arrival order.
#[tokio::test]
async fn failed_shutdown_returns_interleaved_queued_input_and_joins_the_model() {
    let (control, store) = StoreControl::pair();
    let finished = Arc::new(AtomicBool::new(false));
    let driver = FakeDriver::new([Script::WaitForCancellation {
        started: Arc::new(Notify::new()),
        finished: Arc::clone(&finished),
    }]);
    let mut runtime = runtime(store, driver).await;
    let _announcement = runtime.try_next_event();
    runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("open turn: {error}"));
    for input in [
        Input::Submitted {
            text: "turn one".to_owned(),
        },
        Input::Steered {
            text: "step one".to_owned(),
        },
        Input::Submitted {
            text: "turn two".to_owned(),
        },
    ] {
        runtime
            .submit(agent_id(), input)
            .await
            .unwrap_or_else(|error| panic!("queue shutdown input: {error}"));
    }
    control.fail_after(1, false);

    let report = runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("failed shutdown must return ownership: {error}"));

    assert_eq!(
        report
            .undelivered
            .iter()
            .map(|input| input.text.as_str())
            .collect::<Vec<_>>(),
        ["turn one", "step one", "turn two"]
    );
    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::NotWritten)
    );
    assert!(!runtime.has_active_work());
    assert!(finished.load(Ordering::SeqCst));
}
