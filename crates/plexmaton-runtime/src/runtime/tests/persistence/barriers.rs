use super::*;
use crate::DelegatedProjectionRefusal;
use crate::runtime::tests::{
    compaction::{CompactionDriver, SummaryScript, large_answer},
    finish_active, text_delta,
};
use plexmaton_core::{ConversationEvent, ConversationEventEnvelope, TranscriptItemId};

fn delegated_marker(marker: &str) -> ConversationEvent {
    ConversationEvent::RuntimeWarning {
        agent_id: agent_id(),
        item_id: TranscriptItemId::new(format!("stage-7-1-{marker}"))
            .unwrap_or_else(|error| panic!("delegated marker identity: {error}")),
        message: marker.to_owned(),
    }
}

fn drain_announcement(runtime: &mut LiveRuntime) {
    assert!(matches!(
        runtime.try_next_event(),
        Some(ConversationEventEnvelope {
            event: ConversationEvent::AgentCreated { .. },
            ..
        })
    ));
}

async fn collect_events(runtime: &mut LiveRuntime, count: usize) -> Vec<ConversationEventEnvelope> {
    let mut events = Vec::with_capacity(count);
    while events.len() < count {
        let update = tokio::time::timeout(Duration::from_secs(5), runtime.next_update())
            .await
            .unwrap_or_else(|_| panic!("runtime did not publish staged event {}", events.len() + 1))
            .unwrap_or_else(|error| panic!("collect staged event: {error}"));
        match update {
            RuntimeUpdate::Event(event) => events.push(event),
            RuntimeUpdate::Report(report) => {
                panic!("unexpected report while collecting staged events: {report:?}")
            }
            RuntimeUpdate::Finished => panic!("runtime finished while collecting staged events"),
        }
    }
    events
}

fn assert_submission_events_precede_markers(events: &[ConversationEventEnvelope]) {
    assert_eq!(
        events.len(),
        6,
        "one submission and two delegated markers are expected"
    );
    assert!(matches!(
        &events[0].event,
        ConversationEvent::TranscriptItemStarted { .. }
    ));
    assert!(matches!(
        &events[1].event,
        ConversationEvent::TranscriptDelta { text, .. } if text == "keep this exact draft"
    ));
    assert!(matches!(
        &events[2].event,
        ConversationEvent::TranscriptItemFinalized { .. }
    ));
    assert!(matches!(
        &events[3].event,
        ConversationEvent::AgentStatusChanged {
            status: plexmaton_core::AgentStatus::Running,
            ..
        }
    ));
    assert!(matches!(
        &events[4].event,
        ConversationEvent::RuntimeWarning { message, .. } if message == "delegated-a"
    ));
    assert!(matches!(
        &events[5].event,
        ConversationEvent::RuntimeWarning { message, .. } if message == "delegated-b"
    ));

    let sequences = events
        .iter()
        .map(|event| event.sequence.get())
        .collect::<Vec<_>>();
    assert_eq!(sequences, [2, 3, 4, 5, 6, 7]);
    for marker in ["delegated-a", "delegated-b"] {
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    matches!(
                        &event.event,
                        ConversationEvent::RuntimeWarning { message, .. } if message == marker
                    )
                })
                .count(),
            1,
            "delegated marker {marker} must appear exactly once"
        );
    }
}

/// JRN-7/ENT-1/ENT-3: a cancelled append waiter refuses delegated projection without retaining or
/// numbering it. The caller retries after acknowledgement, preserving consecutive exact-once order.
#[tokio::test]
async fn cancelled_submit_publishes_staged_events_before_projected_delegation() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    // A failed assertion must release the synchronous writer before the runtime is dropped.
    let _release = ReleaseGateOnDrop(&control.gate);
    drain_announcement(&mut runtime);
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

    let delegated_a = delegated_marker("delegated-a");
    let delegated_b = delegated_marker("delegated-b");
    for _ in 0..64 {
        assert_eq!(
            runtime.project_delegated(&delegated_a),
            Err(DelegatedProjectionRefusal::PendingCommit),
            "a stalled append admits no unbounded projection suffix"
        );
    }
    assert_eq!(
        runtime.project_delegated(&delegated_b),
        Err(DelegatedProjectionRefusal::PendingCommit)
    );
    assert!(
        runtime.acknowledged_conversation().is_none(),
        "the blocked submission remains unacknowledged"
    );
    assert!(
        runtime.try_next_event().is_none(),
        "projected delegated event escaped before append acknowledgement"
    );
    assert!(
        driver.calls().await.is_empty(),
        "provider started before submission acknowledgement"
    );

    control.gate.release();
    let mut events = collect_events(&mut runtime, 4).await;
    assert_eq!(runtime.delegated_projection_refusal(), None);
    runtime
        .project_delegated(&delegated_a)
        .expect("project first retained delegated event after acknowledgement");
    runtime
        .project_delegated(&delegated_b)
        .expect("project second retained delegated event after acknowledgement");
    events.extend(collect_events(&mut runtime, 2).await);
    assert_submission_events_precede_markers(&events);
    assert_eq!(
        driver.calls().await.len(),
        1,
        "provider starts exactly once after the append is acknowledged"
    );
}

/// JRN-7/ENT-1/ENT-3: a definite append failure changes caller-retained delegated projection from
/// transient backpressure to reopen-required, and no reaction or delegated event can escape.
#[tokio::test]
async fn failed_cancelled_submit_reports_before_projected_delegation() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _release = ReleaseGateOnDrop(&control.gate);
    drain_announcement(&mut runtime);
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

    let delegated_a = delegated_marker("delegated-a");
    let delegated_b = delegated_marker("delegated-b");
    assert_eq!(
        runtime.project_delegated(&delegated_a),
        Err(DelegatedProjectionRefusal::PendingCommit)
    );
    assert_eq!(
        runtime.project_delegated(&delegated_b),
        Err(DelegatedProjectionRefusal::PendingCommit)
    );
    assert!(
        runtime.acknowledged_conversation().is_none(),
        "the blocked submission remains unacknowledged"
    );
    control.gate.release();

    let first = tokio::time::timeout(Duration::from_secs(5), runtime.next_update())
        .await
        .unwrap_or_else(|_| panic!("runtime did not report the definite append failure"))
        .unwrap_or_else(|error| panic!("resume definite append failure: {error}"));
    let report = match first {
        RuntimeUpdate::Report(report) => report,
        other => panic!("first update after release was not the persistence report: {other:?}"),
    };
    assert!(
        runtime.try_next_event().is_none(),
        "deferred delegated or reaction event escaped after definite append failure"
    );
    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::NotWritten)
    );
    assert!(matches!(
        report.undelivered.as_slice(),
        [input]
            if input.text == "keep this exact draft"
                && input.reason == UndeliveredReason::PersistenceFailed
    ));
    assert!(
        driver.calls().await.is_empty(),
        "definite failure started a provider"
    );
    assert!(runtime.acknowledged_conversation().is_none());
    assert_eq!(
        runtime.project_delegated(&delegated_a),
        Err(DelegatedProjectionRefusal::PersistenceFailed),
        "the failed runtime cannot consume the retained durable fact"
    );
}

/// JRN-7/ENT-1/ENT-3: typed uncertainty follows the same refusal and no-event/no-effect boundary.
/// The ControlledStore error models an unknown write outcome; it does not prove write-then-error
/// bytes reached storage.
#[tokio::test]
async fn uncertain_cancelled_submit_reports_before_projected_delegation() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _release = ReleaseGateOnDrop(&control.gate);
    drain_announcement(&mut runtime);
    control.block_after(1);
    control.fail_after(1, true);

    {
        let entered = control.gate.entered.notified();
        let submit = runtime.submit(agent_id(), submission());
        tokio::pin!(submit);
        tokio::select! {
            result = &mut submit => panic!("submit completed before injected uncertain failure: {result:?}"),
            () = entered => {}
        }
    }

    let delegated_a = delegated_marker("delegated-a");
    let delegated_b = delegated_marker("delegated-b");
    assert_eq!(
        runtime.project_delegated(&delegated_a),
        Err(DelegatedProjectionRefusal::PendingCommit)
    );
    assert_eq!(
        runtime.project_delegated(&delegated_b),
        Err(DelegatedProjectionRefusal::PendingCommit)
    );
    assert!(
        runtime.acknowledged_conversation().is_none(),
        "the blocked submission remains unacknowledged"
    );
    control.gate.release();

    let first = tokio::time::timeout(Duration::from_secs(5), runtime.next_update())
        .await
        .unwrap_or_else(|_| panic!("runtime did not report the uncertain append failure"))
        .unwrap_or_else(|error| panic!("resume uncertain append failure: {error}"));
    let report = match first {
        RuntimeUpdate::Report(report) => report,
        other => panic!("first update after release was not the persistence report: {other:?}"),
    };
    assert!(
        runtime.try_next_event().is_none(),
        "deferred delegated or reaction event escaped after uncertain append failure"
    );
    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::OutcomeUnknown)
    );
    assert!(matches!(
        report.undelivered.as_slice(),
        [input]
            if input.text == "keep this exact draft"
                && input.reason == UndeliveredReason::PersistenceFailed
    ));
    assert!(
        driver.calls().await.is_empty(),
        "uncertain failure started a provider"
    );
    assert!(runtime.acknowledged_conversation().is_none());
    assert_eq!(
        runtime.project_delegated(&delegated_a),
        Err(DelegatedProjectionRefusal::PersistenceFailed),
        "the uncertain runtime cannot consume the retained durable fact"
    );
}

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

/// TIM-2/JRN-7: the request-specific authorization has its own acknowledgement before dispatch.
#[tokio::test]
async fn model_dispatch_waits_for_its_request_authorization_ack() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    control.block_on_payload(BlockPayload::RequestAuthorized);

    {
        let entered = control.gate.entered.notified();
        let submit = runtime.submit(agent_id(), submission());
        tokio::pin!(submit);
        tokio::select! {
            result = &mut submit => panic!("submit completed before authorization ack: {result:?}"),
            () = entered => {}
        }
        assert!(driver.calls().await.is_empty());
        assert!(
            control
                .records
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .any(|record| matches!(
                    record,
                    JournalRecord::AppendEntry { entry, .. }
                        if matches!(&entry.payload, JournalEntryPayload::TurnStarted { .. })
                )),
            "the semantic user boundary commits before request authorization"
        );

        control.gate.release();
        submit
            .await
            .unwrap_or_else(|error| panic!("finish authorized submit: {error}"));
    }
    assert_eq!(driver.calls().await.len(), 1);
    assert!(runtime.has_active_model());
    assert!(
        control
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|record| matches!(
                record,
                JournalRecord::RequestAttemptAuthorized { fact, .. }
                    if fact.environment() == &driver.environment
            ))
    );
}

/// CPL-6–CPL-8/JRN-7: authorization, collected terminal, and checkpoint each cross their own
/// acknowledgement barrier before the next effect or refreshed agent request can begin.
#[tokio::test]
async fn compaction_attempt_and_checkpoint_each_wait_for_ack_before_continuation() {
    let (control, store) = StoreControl::pair();
    let driver = CompactionDriver::new(
        [
            Script::Events(vec![
                text_delta(&large_answer()),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
        ],
        [SummaryScript::Complete(
            "bounded checkpoint facts".repeat(8),
        )],
    );
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    while runtime.try_next_event().is_some() {}
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "seed".to_owned(),
            },
        )
        .await
        .expect("seed submit");
    finish_active(&mut runtime).await;
    driver.enable();
    control.block_on_payload(BlockPayload::CompactionAuthorized);

    {
        let entered = control.gate.entered.notified();
        let submit = runtime.submit(
            agent_id(),
            Input::Submitted {
                text: "continue".to_owned(),
            },
        );
        tokio::pin!(submit);
        tokio::select! {
            result = &mut submit => panic!("submit completed before compaction authorization ack: {result:?}"),
            () = entered => {}
        }
        assert_eq!(driver.summary_call_count(), 0);
        control.gate.release();
        submit.await.expect("release compaction authorization");
    }
    assert_eq!(driver.summary_call_count(), 1);

    control.block_on_payload(BlockPayload::CompactionFinished);
    drive_until_store_blocks(&mut runtime, &control).await;
    assert!(
        control
            .records
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .all(|record| !matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(entry.payload, JournalEntryPayload::CompactionCheckpoint { .. })
            ))
    );
    control.gate.release();
    while control
        .records
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .all(|record| !matches!(record, JournalRecord::CompactionAttemptFinished { .. }))
    {
        tokio::task::yield_now().await;
    }

    control.block_on_payload(BlockPayload::CompactionCheckpoint);
    drive_until_store_blocks(&mut runtime, &control).await;
    assert_eq!(
        driver.agent_calls().await.len(),
        1,
        "agent remains undispatched"
    );
    control.gate.release();

    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.has_active_work() {
            runtime
                .next_update()
                .await
                .expect("finish checkpoint barrier");
        }
    })
    .await
    .expect("checkpoint barrier continuation timed out");
    assert_eq!(driver.agent_calls().await.len(), 2);
}

/// TIM-2/TIM-3/JRN-7: terminal accounting commits before stop can publish canonical output.
#[tokio::test]
async fn model_terminal_audit_commits_before_semantic_completion() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::Events(vec![
        ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 0),
            delta: "finished answer".to_owned(),
        },
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let mut runtime = runtime(store, driver).await;
    while runtime.try_next_event().is_some() {}
    runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("open model turn: {error}"));
    control.block_on_payload(BlockPayload::RequestFinished);

    drive_until_store_blocks(&mut runtime, &control).await;
    assert!(runtime.has_active_model());
    assert!(
        control
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .all(|record| !matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(&entry.payload, JournalEntryPayload::AssistantOutput { .. })
            )),
        "assistant completion entered storage before terminal accounting"
    );

    control.gate.release();
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.has_active_work() {
            let _update = runtime
                .next_update()
                .await
                .unwrap_or_else(|error| panic!("finish terminal barrier: {error}"));
        }
    })
    .await
    .unwrap_or_else(|_| panic!("terminal barrier did not settle"));

    let records = control
        .records
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let terminal = records
        .iter()
        .position(|record| matches!(record, JournalRecord::RequestAttemptFinished { .. }))
        .unwrap_or_else(|| panic!("request terminal was not stored"));
    let output = records
        .iter()
        .position(|record| {
            matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(&entry.payload, JournalEntryPayload::AssistantOutput { .. })
            )
        })
        .unwrap_or_else(|| panic!("assistant output was not stored"));
    assert!(terminal < output);
}

/// JRN-7: tool admission and execution each wait for their own lifecycle append.
#[tokio::test]
async fn tool_effects_start_only_after_their_transition_is_acknowledged() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::Events(vec![
        ModelEvent::Called {
            position: ModelOutputPosition::new(0, 0),
            call: ToolCall {
                call_id: ToolCallId::new("durable-read")
                    .unwrap_or_else(|error| panic!("call id: {error}")),
                name: "read_file".to_owned(),
                arguments: serde_json::json!({ "path": "Cargo.toml" }).to_string(),
            },
        },
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
        ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 0),
            delta: "first answer".to_owned(),
        },
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
        ModelEvent::Called {
            position: ModelOutputPosition::new(0, 0),
            call: ToolCall {
                call_id: ToolCallId::new("unknown-steering-tool")
                    .unwrap_or_else(|error| panic!("call id: {error}")),
                name: "unknown_tool".to_owned(),
                arguments: "{}".to_owned(),
            },
        },
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
        ModelEvent::Called {
            position: ModelOutputPosition::new(0, 0),
            call: duplicate.clone(),
        },
        ModelEvent::Called {
            position: ModelOutputPosition::new(1, 0),
            call: duplicate,
        },
        ModelEvent::TextDelta {
            position: ModelOutputPosition::new(2, 0),
            delta: "must not hide the report".to_owned(),
        },
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
