use super::*;

/// JRN-7: no provider effect starts before every record in its transition is acknowledged.
#[tokio::test]
async fn durable_transition_starts_no_effect_before_append_ack() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    // TIM-1 makes the user item and Running boundary one record. Hold that record so the test
    // fails if the provider starts before its one atomic start is acknowledged.
    control.block_after(1);
    {
        let entered = control.gate.entered.notified();
        let submit = runtime.submit(agent_id(), submission());
        tokio::pin!(submit);
        tokio::select! {
            result = &mut submit => panic!("submit completed before append ack: {result:?}"),
            () = entered => {}
        }
        assert!(driver.calls().await.is_empty());

        control.gate.release();
        submit
            .as_mut()
            .await
            .unwrap_or_else(|error| panic!("finish submission: {error}"));
    }
    assert!(runtime.has_active_model());
}

/// JRN-7: tool admission and execution each wait for their own lifecycle append.
#[tokio::test]
async fn tool_effects_start_only_after_their_transition_is_acknowledged() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::Events(vec![
        ModelEvent::Called(ToolCall {
            call_id: ToolCallId::new("durable-read")
                .unwrap_or_else(|error| panic!("call id: {error}")),
            name: "read_file".to_owned(),
            arguments: serde_json::json!({ "path": "Cargo.toml" }).to_string(),
        }),
        ModelEvent::Stopped(StopReason::ToolCalls),
    ])]);
    let mut runtime = runtime(store, driver).await;
    while runtime.try_next_event().is_some() {}
    runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("open tool turn: {error}"));
    while runtime.try_next_event().is_some() {}

    control.block_on_payload(BlockPayload::ToolRequested);
    drive_until_store_blocks(&mut runtime, &control).await;
    assert!(
        runtime.tools.is_empty(),
        "tool admission started before ack"
    );
    control.gate.release();
    let _event = runtime
        .next_update()
        .await
        .unwrap_or_else(|error| panic!("release admission barrier: {error}"));
    assert!(
        !runtime.tools.is_empty(),
        "ack did not start tool admission"
    );

    control.block_on_payload(BlockPayload::ToolStatus(ToolCallStatus::Running));
    drive_until_store_blocks(&mut runtime, &control).await;
    assert!(
        runtime.tools.is_empty(),
        "tool execution started before ack"
    );
    control.gate.release();
    let _event = runtime
        .next_update()
        .await
        .unwrap_or_else(|error| panic!("release execution barrier: {error}"));
    assert!(
        !runtime.tools.is_empty(),
        "ack did not start tool execution"
    );

    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown tool barrier fixture: {error}"));
}

/// JRN-7/LOOP-6: appending to either live input queue needs no journal write or rollback clone.
#[tokio::test]
async fn running_submission_and_steering_enter_their_queues_without_a_commit() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, driver).await;
    let _announcement = runtime.try_next_event();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "first turn".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("open turn: {error}"));
    let records_before = control
        .records
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .len();

    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "next turn".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("queue next turn: {error}"));
    runtime
        .submit(
            agent_id(),
            Input::Steered {
                text: "next step".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("queue next step: {error}"));

    assert_eq!(
        control
            .records
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len(),
        records_before
    );
    assert_eq!(
        runtime.agent.queued_for_next_turn().collect::<Vec<_>>(),
        ["next turn"]
    );
    assert_eq!(
        runtime.agent.queued_for_next_step().collect::<Vec<_>>(),
        ["next step"]
    );
}

/// JRN-7/LOOP-6: an internally claimed next-turn message returns through the observable update.
#[tokio::test]
async fn failed_claim_of_queued_next_turn_text_returns_the_exact_input() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::Events(vec![
        ModelEvent::TextDelta("first answer".to_owned()),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "first turn".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("open first turn: {error}"));
    runtime
        .submit(
            agent_id(),
            Input::Steered {
                text: "earlier steering".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("queue steering: {error}"));
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "queued next turn".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("queue next turn: {error}"));
    control.fail_on_text("queued next turn");

    let report = await_report(&mut runtime).await;

    assert_eq!(
        report
            .undelivered
            .iter()
            .map(|input| (input.text.as_str(), input.reason))
            .collect::<Vec<_>>(),
        [
            ("earlier steering", UndeliveredReason::PersistenceFailed),
            ("queued next turn", UndeliveredReason::PersistenceFailed),
        ]
    );
    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::OutcomeUnknown)
    );
    assert_eq!(
        driver.calls().await.len(),
        1,
        "no second model effect started"
    );
}

/// JRN-7/LOOP-6: next-step steering claimed after a tool result has the same ownership path.
#[tokio::test]
async fn failed_claim_of_queued_steering_returns_the_exact_input() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::Events(vec![
        ModelEvent::Called(ToolCall {
            call_id: ToolCallId::new("unknown-steering-tool")
                .unwrap_or_else(|error| panic!("call id: {error}")),
            name: "unknown_tool".to_owned(),
            arguments: "{}".to_owned(),
        }),
        ModelEvent::Stopped(StopReason::ToolCalls),
    ])]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "use a tool".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("open tool turn: {error}"));
    runtime
        .submit(
            agent_id(),
            Input::Steered {
                text: "queued next step".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("queue steering: {error}"));
    control.fail_on_text("queued next step");

    let report = await_report(&mut runtime).await;

    assert!(matches!(
        report.undelivered.as_slice(),
        [input]
            if input.text == "queued next step"
                && input.reason == UndeliveredReason::PersistenceFailed
    ));
    assert_eq!(
        driver.calls().await.len(),
        1,
        "no next-step model effect started"
    );
    assert!(
        !runtime.has_active_work(),
        "journal failure joined tool work"
    );
}

/// JRN-7: a provider burst stops at the durable failure so its ownership report stays reachable.
#[tokio::test]
async fn a_burst_stops_after_journal_failure_and_yields_the_queued_input_report() {
    let duplicate = ToolCall {
        call_id: ToolCallId::new("burst-duplicate")
            .unwrap_or_else(|error| panic!("call id: {error}")),
        name: "read_file".to_owned(),
        arguments: serde_json::json!({ "path": "Cargo.toml" }).to_string(),
    };
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::Events(vec![
        ModelEvent::Called(duplicate.clone()),
        ModelEvent::Called(duplicate),
        ModelEvent::TextDelta("must not hide the report".to_owned()),
    ])]);
    let mut runtime = runtime(store, driver).await;
    while runtime.try_next_event().is_some() {}
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "open burst turn".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("open burst turn: {error}"));
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "return this queued turn".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("queue follow-up: {error}"));
    control.fail_after(1, false);

    let report = await_report(&mut runtime).await;

    assert!(matches!(
        report.undelivered.as_slice(),
        [input]
            if input.text == "return this queued turn"
                && input.reason == UndeliveredReason::PersistenceFailed
    ));
    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::NotWritten)
    );
}
