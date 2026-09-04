use super::*;

struct IncrementingClock(AtomicUsize);

impl crate::runtime::clock::WallClock for IncrementingClock {
    fn now(&self) -> plexmaton_agent::UnixMillis {
        let value = self.0.fetch_add(1, Ordering::SeqCst);
        plexmaton_agent::UnixMillis::new(
            u64::try_from(value).unwrap_or_else(|error| panic!("clock value: {error}")),
        )
    }
}

/// TIM-1/JRN-7: input is timestamped and runtime-owned before an older commit can suspend it.
#[tokio::test]
async fn cancelled_submit_behind_an_older_commit_keeps_its_arrival_time_and_text() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([
        Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        Script::EndWithoutTerminal,
    ]);
    let clock = Arc::new(IncrementingClock(AtomicUsize::new(100)));
    let mut runtime = runtime_with_clock(store, driver, clock).await;
    assert_eq!(
        runtime.agent.journal().created_at_unix_ms(),
        plexmaton_agent::UnixMillis::new(100)
    );
    let _announcement = runtime.try_next_event();
    control.block_after(1);

    {
        let entered = control.gate.entered.notified();
        let first = runtime.submit(agent_id(), submission());
        tokio::pin!(first);
        tokio::select! {
            result = &mut first => panic!("first submit completed before append ack: {result:?}"),
            () = entered => {}
        }
    }
    {
        let second = runtime.submit(
            agent_id(),
            Input::Submitted {
                text: "retained behind the first commit".to_owned(),
            },
        );
        tokio::pin!(second);
        tokio::select! {
            biased;
            result = &mut second => panic!("second submit completed while first was blocked: {result:?}"),
            () = tokio::task::yield_now() => {}
        }
    }
    control.gate.release();

    loop {
        let has_second = runtime.agent.journal().records().iter().any(|record| {
            matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(
                        &entry.payload,
                        JournalEntryPayload::TurnStarted {
                            text,
                            accepted_at,
                            ..
                        } if text == "retained behind the first commit"
                            && *accepted_at == plexmaton_agent::UnixMillis::new(102)
                    )
            )
        });
        if has_second {
            break;
        }
        tokio::time::timeout(Duration::from_secs(5), runtime.next_update())
            .await
            .unwrap_or_else(|_| panic!("retained submission did not reach its boundary"))
            .unwrap_or_else(|error| panic!("drive retained submission: {error}"));
    }
}

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

/// JRN-7: a cancelled interrupt poll retains its terminal commit and later joins the model.
#[tokio::test]
async fn cancelled_interrupt_commit_still_joins_the_model_after_resume() {
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
            result = &mut interrupt => panic!("interrupt completed before terminal ack: {result:?}"),
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

/// TIM-2/JRN-7: a cancelled model-end poll keeps its owner until attempt accounting commits.
#[tokio::test]
async fn cancelled_model_end_during_attempt_terminal_append_keeps_the_active_owner() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, driver).await;
    // Drop before runtime: failed assertions must unblock its synchronous writer join.
    let _release = ReleaseGateOnDrop(&control.gate);
    while runtime.try_next_event().is_some() {}
    runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("open turn: {error}"));
    while runtime.try_next_event().is_some() {}
    control.block_on_payload(BlockPayload::RequestFinished);

    {
        let entered = tokio::time::timeout(Duration::from_secs(5), control.gate.entered.notified());
        let update = runtime.next_update();
        tokio::pin!(update);
        tokio::select! {
            result = &mut update => panic!("model end completed before attempt terminal ack: {result:?}"),
            result = entered => result.expect("request terminal append never reached the test gate"),
        }
    }
    assert!(runtime.has_active_model());
    assert!(
        !control
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|record| { matches!(record, JournalRecord::RequestAttemptFinished { .. }) })
    );
    control.gate.release();
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.has_active_model() {
            runtime.next_update().await.expect("resume model end");
        }
    })
    .await
    .expect("retained model end did not resume");
    assert_eq!(
        control
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|record| { matches!(record, JournalRecord::RequestAttemptFinished { .. }) })
            .count(),
        1,
        "resuming the cancelled poll commits the terminal exactly once"
    );
}

/// JRN-7: a failing barrier assertion must not deadlock the owned writer's cleanup.
#[tokio::test]
async fn a_panicking_barrier_test_releases_the_writer_before_runtime_drop() {
    use futures_util::FutureExt as _;

    let (control, store) = StoreControl::pair();
    let result = std::panic::AssertUnwindSafe(async {
        let mut runtime = runtime(store, FakeDriver::new([Script::EndWithoutTerminal])).await;
        let _release = ReleaseGateOnDrop(&control.gate);
        runtime
            .submit(agent_id(), submission())
            .await
            .expect("open turn");
        while runtime.try_next_event().is_some() {}
        control.block_on_payload(BlockPayload::RequestFinished);
        tokio::time::timeout(
            Duration::from_secs(5),
            drive_until_store_blocks(&mut runtime, &control),
        )
        .await
        .expect("request terminal append never reached the test gate");
        panic!("injected barrier assertion failure");
    })
    .catch_unwind()
    .await;

    assert!(result.is_err_and(|error| {
        error.downcast_ref::<&str>() == Some(&"injected barrier assertion failure")
    }));
    assert!(
        control.dropped.load(Ordering::SeqCst),
        "writer was joined during unwinding"
    );
    assert!(
        control
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|record| { matches!(record, JournalRecord::RequestAttemptFinished { .. }) }),
        "unwinding released the accepted append; the writer did not merely time out"
    );
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

/// JRN-7/LOOP-6: a cancelled shutdown owns its cleanup and refuses later text unchanged.
#[tokio::test]
async fn submit_after_cancelled_shutdown_returns_text_without_a_record_or_effect() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
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
    let records_before = runtime.agent.journal().records().len();
    let calls_before = driver.calls().await.len();
    let report = runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "return after shutdown".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("shutdown refusal is a report: {error}"));
    assert!(matches!(
        report.undelivered.as_slice(),
        [input]
            if input.text == "return after shutdown"
                && input.reason == UndeliveredReason::Shutdown
    ));
    assert_eq!(runtime.agent.journal().records().len(), records_before);
    assert_eq!(driver.calls().await.len(), calls_before);

    control.gate.release();
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("resume shutdown: {error}"));
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
