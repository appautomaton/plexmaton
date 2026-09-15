use plexmaton_agent::Input;

use super::*;
use crate::{UserInputRequest, UserTargetInputRequest};

/// COL-3/SCH-4: only the owner-issued target reaches the exact child after durable Handoff.
#[tokio::test]
async fn col_3_handoff_unlocks_only_the_authenticated_owned_child_input() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-user-input");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let first_driver = FakeDriver::new([
        Script::WaitForCancellation {
            started: Arc::clone(&started),
            finished: Arc::clone(&cancelled),
        },
        Script::Events(vec![
            text_delta("child answer"),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
    ]);
    let second_driver = FakeDriver::new([Script::Events(vec![
        text_delta("idle child answer"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let (first, first_request) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "one",
        Arc::clone(&first_driver),
    )
    .await;
    let (second, _) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "two",
        Arc::clone(&second_driver),
    )
    .await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(2).expect("limits"));
    owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind authenticated Main ingress");
    owner.register(first).await.expect("register first child");
    owner.register(second).await.expect("register second child");
    owner.next_update().await.expect("first announcement");
    owner.next_update().await.expect("second announcement");
    let targets = owner
        .register_collaboration_targets()
        .await
        .expect("register canonical targets");
    let first_target = user_target(&targets, "one");
    let second_target = user_target(&targets, "two");
    let before_handoff = owner
        .activate_user_target(&first_target)
        .await
        .expect_err("Main control refuses direct child input");
    assert!(matches!(
        before_handoff,
        crate::UserInputRefusal::ControlledByMain
    ));
    assert!(first_driver.calls().await.is_empty());
    let stale_main_request = first_request.clone();
    owner
        .schedule(first_request)
        .await
        .expect("start Main-owned child work");
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .expect("active Main turn started");
    let handoff = CollaborationAttempt {
        id: item("handoff-one-to-user"),
        event: CollaborationEvent::HandoffCompleted {
            delegation: named_delegation("one"),
            expected: DelegationRevision(0),
            author: endpoint("main"),
        },
    };
    owner
        .handoff(handoff.clone())
        .await
        .expect("durable Handoff");
    assert!(cancelled.load(Ordering::SeqCst));
    assert!(matches!(
        owner
            .schedule(stale_main_request)
            .await
            .expect_err("Main admission remains closed after Handoff")
            .source(),
        OwnedSchedulingError::HandoffPending
    ));
    let first_ticket = owner
        .activate_user_target(&first_target)
        .await
        .expect("issue exact live User input ticket");
    owner
        .submit_user_input(UserInputRequest::new(
            first_ticket.clone(),
            Input::Submitted {
                text: "continue child".into(),
            },
            None,
        ))
        .await
        .expect("User-controlled input");
    let calls = first_driver.calls().await;
    assert_eq!(calls.len(), 2);
    assert!(calls[1].request.atoms.iter().any(|atom| {
        matches!(atom.value(), ContextAtomValue::User { text } if text == "continue child")
    }));
    let duplicate = owner
        .handoff(handoff)
        .await
        .expect("exact Handoff retry returns its durable receipt");
    assert!(
        duplicate.stopped.is_none(),
        "an exact durable retry must not interrupt User-owned work"
    );
    let stale_ticket = first_ticket
        .with_generation_for_test(RunnerGeneration::new(999_999).expect("stale generation"));
    assert!(matches!(
        owner
            .begin_user_input(UserInputRequest::new(
                stale_ticket,
                Input::Submitted {
                    text: "stale runner".into(),
                },
                None,
            ))
            .await
            .expect_err("stale runner ticket fails before input")
            .reason(),
        crate::UserInputRefusal::StaleTicket
    ));
    let still_main = owner
        .activate_user_target(&second_target)
        .await
        .expect_err("another child remains Main-controlled");
    assert!(matches!(
        still_main,
        crate::UserInputRefusal::ControlledByMain
    ));
    assert!(second_driver.calls().await.is_empty());
    owner.begin_shutdown().await.expect("begin shutdown");
    while owner.next_update().await.is_some() {}
    owner.finish_shutdown().await.expect("join owner");
}

/// COL-3/SCH-2: idle transfer admits its child and a pending Stop blocks any restart.
#[tokio::test]
async fn col_3_idle_handoff_opens_user_input_until_owned_stop_begins() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-idle-user-input");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let driver = FakeDriver::new([Script::Events(vec![
        text_delta("idle child answer"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let (runtime, _) =
        bound_runtime(&directory, &workspace, &writer, "two", Arc::clone(&driver)).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main ingress");
    owner.register(runtime).await.expect("register idle child");
    owner.next_update().await.expect("initial announcement");
    let target = owner
        .register_collaboration_targets()
        .await
        .expect("register targets")
        .into_iter()
        .find(|target| target.worker() == &endpoint("two"))
        .expect("second target")
        .user_input_target();
    owner
        .handoff(CollaborationAttempt {
            id: item("handoff-idle-two"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: named_delegation("two"),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("idle Handoff");
    let ticket = owner
        .activate_user_target(&target)
        .await
        .expect("issue idle child input ticket");
    owner
        .submit_user_input(UserInputRequest::new(
            ticket.clone(),
            Input::Submitted {
                text: "idle continuation".into(),
            },
            None,
        ))
        .await
        .expect("idle child accepts input after Handoff");
    assert_eq!(driver.calls().await.len(), 1);
    owner
        .begin_stop(&endpoint("two").conversation)
        .expect("begin exact child Stop");
    let during_stop = owner
        .begin_user_input(UserInputRequest::new(
            ticket,
            Input::Submitted {
                text: "must not restart".into(),
            },
            None,
        ))
        .await
        .expect_err("Stop closes new child input admission until it settles");
    assert!(matches!(
        during_stop.reason(),
        crate::UserInputRefusal::InProgress
    ));
    assert_eq!(driver.calls().await.len(), 1);

    owner.begin_shutdown().await.expect("begin shutdown");
    while owner.next_update().await.is_some() {}
    owner.finish_shutdown().await.expect("join owner");
}

/// SCH-2/SCH-4: Stop owns target input before cold activation and returns its exact draft.
#[tokio::test]
async fn sch_2_stop_cancels_queued_cold_target_input_before_runner_activation() {
    let directory = Directory::new();
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main ingress");
    let target = owner
        .register_collaboration_targets()
        .await
        .expect("register targets")
        .into_iter()
        .find(|target| target.worker() == &endpoint("one"))
        .expect("first target")
        .user_input_target();
    owner
        .handoff(CollaborationAttempt {
            id: item("handoff-before-queued-stop"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: named_delegation("one"),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("cold Handoff");
    owner
        .begin_user_target_input(UserTargetInputRequest::new(
            target.clone(),
            Input::Submitted {
                text: "return before activation".into(),
            },
            Some("review".into()),
        ))
        .expect("queue exact target input");
    let conversation = endpoint("one").conversation;
    owner
        .begin_stop(&conversation)
        .expect("Stop owns queued cold input without a runner");
    let second = owner
        .begin_user_target_input(UserTargetInputRequest::new(
            target,
            Input::Submitted {
                text: "second rapid draft".into(),
            },
            None,
        ))
        .expect_err("unpublished interrupted input keeps the target lane occupied");
    assert!(matches!(
        second.reason(),
        crate::UserInputRefusal::InProgress
    ));
    assert_eq!(
        second
            .into_undelivered(plexmaton_agent::UndeliveredReason::QueueFull)
            .expect("second rapid draft remains returnable")
            .text,
        "second rapid draft"
    );
    let activity = owner
        .next_activity()
        .await
        .expect("queued-input Stop settlement");
    let crate::OwnedCollaborationActivity::UserInput(settlement) = activity else {
        panic!("queued cold input must settle through owner input activity");
    };
    assert_eq!(&settlement.worker().conversation, &conversation);
    let returned = settlement
        .into_outcome()
        .expect_err("Stop interrupts queued target input")
        .into_undelivered(plexmaton_agent::UndeliveredReason::Interrupted)
        .expect("queued message remains returnable");
    assert_eq!(returned.text, "return before activation");
    assert_eq!(returned.skill.as_deref(), Some("review"));
    assert_eq!(
        returned.reason,
        plexmaton_agent::UndeliveredReason::Interrupted
    );
    assert!(
        owner
            .child_session_source(&conversation)
            .await
            .expect("inspect child after queued Stop")
            .is_none(),
        "Stop never cold-activates the child"
    );
    owner.begin_shutdown().await.expect("begin shutdown");
    owner.finish_shutdown().await.expect("finish shutdown");
}

/// SCH-2/SCH-4: an accepted child input is bounded and settles after its caller stops waiting.
#[tokio::test]
async fn sch_2_cancelled_user_input_wait_retains_one_settlement_and_exact_backpressure() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-user-input-cancellation");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let driver = FakeDriver::new([Script::Events(vec![
        text_delta("accepted answer"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let (runtime, _) =
        bound_runtime(&directory, &workspace, &writer, "one", Arc::clone(&driver)).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main ingress");
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial announcement");
    let target = owner
        .register_collaboration_targets()
        .await
        .expect("register targets")
        .into_iter()
        .find(|target| target.worker() == &endpoint("one"))
        .expect("first target")
        .user_input_target();
    owner
        .handoff(CollaborationAttempt {
            id: item("handoff-cancelled-input"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: named_delegation("one"),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("handoff");
    let ticket = owner
        .activate_user_target(&target)
        .await
        .expect("issue User input ticket");

    let conversation = endpoint("one").conversation;
    let release = owner
        .hold_user_input_for_test(&conversation)
        .await
        .expect("hold child input handler");
    let admitted = owner.notify_next_user_input_admission_for_test(&conversation);
    {
        let submitted = owner.submit_user_input(UserInputRequest::new(
            ticket.clone(),
            Input::Submitted {
                text: "accepted once".into(),
            },
            None,
        ));
        tokio::pin!(submitted);
        tokio::select! {
            () = admitted.notified() => {}
            result = &mut submitted => panic!("input settled before cancellation: {result:?}"),
        }
    }
    let busy = owner
        .begin_user_input(UserInputRequest::new(
            ticket,
            Input::Steered {
                text: "retain me".into(),
            },
            Some("review".into()),
        ))
        .await
        .expect_err("one owner-level input remains bounded");
    assert!(matches!(busy.reason(), crate::UserInputRefusal::InProgress));
    let recovered = busy.into_request();
    assert!(matches!(
        recovered.input(),
        Input::Steered { text } if text == "retain me"
    ));
    assert_eq!(recovered.selected_skill(), Some("review"));

    release.notify_one();
    let settled = owner
        .next_update()
        .await
        .expect("abandoned input result remains owner-visible");
    assert!(matches!(
        settled,
        OwnedRunnerUpdate::UserInputSettled {
            outcome,
            ..
        } if outcome.is_ok()
    ));
    assert_eq!(driver.calls().await.len(), 1);

    owner
        .begin_user_input(UserInputRequest::new(
            recovered.ticket().clone(),
            Input::Submitted {
                text: "owned through shutdown".into(),
            },
            None,
        ))
        .await
        .expect("accept input before its caller stops waiting");
    owner
        .begin_shutdown()
        .await
        .expect("shutdown settles the accepted input");
    while owner.next_update().await.is_some() {}
    let shutdown = owner.finish_shutdown().await.expect("join owner");
    assert!(matches!(
        shutdown.settlements(),
        [OwnedShutdownSettlement::UserInput(Ok(_))]
    ));
    assert!(shutdown.runners().iter().any(|(_, report)| {
        report
            .undelivered
            .iter()
            .any(|input| input.text == "owned through shutdown")
    }));
}

/// SCH-2/SCH-4: a failed accepted input is returned and never prevents the requested Stop.
#[tokio::test]
async fn sch_4_failed_user_input_still_stops_active_work_and_returns_the_exact_request() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("failed-user-input-stop");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver = FakeDriver::new([Script::WaitForCancellation {
        started: Arc::clone(&started),
        finished: Arc::clone(&cancelled),
    }]);
    let (runtime, _) =
        bound_runtime(&directory, &workspace, &writer, "one", Arc::clone(&driver)).await;
    let conversation = runtime.conversation_id().clone();
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main ingress");
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial announcement");
    let target = owner
        .register_collaboration_targets()
        .await
        .expect("register targets")
        .into_iter()
        .find(|target| target.worker() == &endpoint("one"))
        .expect("first target")
        .user_input_target();
    owner
        .handoff(CollaborationAttempt {
            id: item("handoff-before-input-failure"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: named_delegation("one"),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("handoff");
    let ticket = owner
        .activate_user_target(&target)
        .await
        .expect("issue User ticket");
    owner
        .submit_user_input(UserInputRequest::new(
            ticket.clone(),
            Input::Submitted {
                text: "run until stopped".into(),
            },
            None,
        ))
        .await
        .expect("start User-owned work");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            tokio::select! {
                () = started.notified() => break,
                update = owner.next_update() => {
                    update.expect("child remains live before provider start");
                }
            }
        }
    })
    .await
    .expect("active User turn started");

    owner.fail_next_user_input_for_test(&conversation);
    owner
        .begin_user_input(UserInputRequest::new(
            ticket,
            Input::Steered {
                text: "return exact failed input".into(),
            },
            Some("review".into()),
        ))
        .await
        .expect("accept injected failure");
    let stopped = tokio::time::timeout(Duration::from_secs(5), owner.stop(&conversation))
        .await
        .expect("Stop deadline after input failure")
        .expect("input failure cannot prevent Stop");
    assert!(cancelled.load(Ordering::SeqCst));
    let failed = stopped
        .user_input
        .expect("Stop retains input outcome")
        .expect_err("injected input failure");
    let returned = failed
        .into_undelivered(plexmaton_agent::UndeliveredReason::Interrupted)
        .expect("failed message remains returnable");
    assert_eq!(returned.text, "return exact failed input");
    assert_eq!(returned.skill.as_deref(), Some("review"));
    assert_eq!(
        returned.reason,
        plexmaton_agent::UndeliveredReason::Interrupted
    );
    assert_eq!(driver.calls().await.len(), 1);
    shutdown_owner(&mut owner).await;
}

/// SCH-2: a different child's Stop settles while one User-input lane remains blocked.
#[tokio::test]
async fn sch_2_cross_child_stop_bypasses_an_unrelated_blocked_user_input() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("cross-child-user-input-stop");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let (first, _) =
        bound_runtime(&directory, &workspace, &writer, "one", FakeDriver::new([])).await;
    let (second, _) =
        bound_runtime(&directory, &workspace, &writer, "two", FakeDriver::new([])).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(2).expect("limits"));
    owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main ingress");
    let first_identity = owner.register(first).await.expect("register first");
    let second_identity = owner.register(second).await.expect("register second");
    owner.next_update().await.expect("first announcement");
    owner.next_update().await.expect("second announcement");
    let targets = owner
        .register_collaboration_targets()
        .await
        .expect("register targets");
    for (name, handoff_id) in [("one", "handoff-blocked-one"), ("two", "handoff-stop-two")] {
        owner
            .handoff(CollaborationAttempt {
                id: item(handoff_id),
                event: CollaborationEvent::HandoffCompleted {
                    delegation: named_delegation(name),
                    expected: DelegationRevision(0),
                    author: endpoint("main"),
                },
            })
            .await
            .expect("handoff child");
    }
    let first_target = targets
        .iter()
        .find(|target| target.worker() == &endpoint("one"))
        .expect("first target")
        .user_input_target();
    let ticket = owner
        .activate_user_target(&first_target)
        .await
        .expect("issue first ticket");
    let release = owner
        .hold_user_input_for_test(&first_identity.endpoint().conversation)
        .await
        .expect("block first input handler");
    owner
        .begin_user_input(UserInputRequest::new(
            ticket,
            Input::Submitted {
                text: "blocked first child".into(),
            },
            None,
        ))
        .await
        .expect("queue blocked first input");
    owner
        .begin_stop(&second_identity.endpoint().conversation)
        .expect("admit second child Stop");

    let stopped = tokio::time::timeout(Duration::from_secs(5), owner.next_update())
        .await
        .expect("different-child Stop deadline")
        .expect("Stop settlement");
    assert!(matches!(
        stopped,
        OwnedRunnerUpdate::StopSettled { identity, outcome }
            if identity == second_identity && outcome.is_ok()
    ));
    release.notify_one();
    let input = owner.next_update().await.expect("first input settlement");
    assert!(matches!(
        input,
        OwnedRunnerUpdate::UserInputSettled { identity, outcome }
            if identity == first_identity && outcome.is_ok()
    ));
    shutdown_owner(&mut owner).await;
}

/// SCH-1/SCH-4: a canceled cold Handoff settles through owner activity without a runner identity.
#[tokio::test]
async fn sch_1_cancelled_cold_handoff_has_one_typed_owner_settlement() {
    let directory = Directory::new();
    let collaboration_path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let attempt = CollaborationAttempt {
        id: item("cancelled-cold-handoff"),
        event: CollaborationEvent::HandoffCompleted {
            delegation: named_delegation("one"),
            expected: DelegationRevision(0),
            author: endpoint("main"),
        },
    };
    let (entered, release) = owner.hold_writer_for_test();
    entered.recv().expect("writer held");
    assert!(
        tokio::time::timeout(Duration::ZERO, owner.handoff(attempt.clone()))
            .await
            .is_err(),
        "caller drops the cold Handoff wait"
    );
    let overlap = CollaborationAttempt {
        id: item("blocked-by-cold-handoff"),
        event: CollaborationEvent::TaskUpdated {
            delegation: named_delegation("two"),
            task: CollaborationText::new("wait for Handoff").expect("task"),
            expected: DelegationRevision(0),
            author: endpoint("main"),
        },
    };
    let refused = owner
        .admit(overlap.clone())
        .await
        .expect_err("a retained Handoff excludes a competing admission");
    assert!(matches!(
        refused,
        CollaborationWriterError::AdmissionInProgress { ref attempt }
            if attempt.as_ref() == &overlap
    ));
    release.send(()).expect("release writer");
    let activity = tokio::time::timeout(Duration::from_secs(5), owner.next_activity())
        .await
        .expect("cold Handoff settlement deadline")
        .expect("cold Handoff owner activity");
    assert!(matches!(
        activity,
        crate::OwnedCollaborationActivity::Handoff(settlement)
            if settlement.delegation() == &named_delegation("one")
                && settlement.outcome().is_ok()
    ));
    owner.begin_shutdown().await.expect("begin shutdown");
    owner.finish_shutdown().await.expect("join owner");
    let reopened = CollaborationFile::open(collaboration_path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .delegation(&named_delegation("one"))
            .expect("delegation")
            .controller,
        DelegationController::User
    );
}

/// SCH-1/SCH-4: a canceled terminal join publishes before its retained cold Handoff.
#[tokio::test]
async fn sch_4_cold_handoff_waits_for_a_cancelled_terminal_join() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("cold-handoff-after-cancelled-join");
    let collaboration_path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let (runtime, _) =
        bound_runtime(&directory, &workspace, &writer, "one", FakeDriver::new([])).await;
    let conversation = runtime.conversation_id().clone();
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main ingress");
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial announcement");
    let (join_entered, join_release) = owner.hold_runner_join_for_test(&conversation);
    owner.panic_runner_for_test(&conversation);
    {
        let terminal = owner.next_update();
        tokio::pin!(terminal);
        tokio::select! {
            () = join_entered.notified() => {}
            update = &mut terminal => panic!("terminal escaped before join: {update:?}"),
        }
    }

    let handoff = CollaborationAttempt {
        id: item("handoff-after-cancelled-join"),
        event: CollaborationEvent::HandoffCompleted {
            delegation: named_delegation("one"),
            expected: DelegationRevision(0),
            author: endpoint("main"),
        },
    };
    let (writer_entered, writer_release) = owner.hold_writer_for_test();
    writer_entered.recv().expect("writer held");
    assert!(
        tokio::time::timeout(Duration::ZERO, owner.handoff(handoff))
            .await
            .is_err(),
        "caller drops the Handoff wait"
    );
    writer_release.send(()).expect("release writer");

    let terminal = {
        let activity = owner.next_activity();
        tokio::pin!(activity);
        tokio::select! {
            () = join_entered.notified() => {}
            update = &mut activity => panic!("Handoff bypassed the retained join: {update:?}"),
        }
        join_release.notify_one();
        tokio::time::timeout(Duration::from_secs(5), &mut activity)
            .await
            .expect("terminal activity deadline")
            .expect("terminal activity")
    };
    assert!(matches!(
        terminal,
        crate::OwnedCollaborationActivity::Runner(OwnedRunnerUpdate::WorkerFailed { .. })
    ));
    let handoff = tokio::time::timeout(Duration::from_secs(5), owner.next_activity())
        .await
        .expect("Handoff activity deadline")
        .expect("Handoff activity");
    assert!(matches!(
        handoff,
        crate::OwnedCollaborationActivity::Handoff(settlement)
            if settlement.outcome().is_ok()
    ));
    owner.begin_shutdown().await.expect("begin shutdown");
    let failure = owner
        .finish_shutdown()
        .await
        .expect_err("runner panic remains visible at shutdown");
    assert!(matches!(
        failure.source(),
        OwnedSchedulingError::Control(crate::OwnedRunnerError::WorkerFailed)
    ));
    let reopened = CollaborationFile::open(collaboration_path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .delegation(&named_delegation("one"))
            .expect("delegation")
            .controller,
        DelegationController::User
    );
}

/// CHB-2/CHB-3: User activation never fabricates replacement history for a missing child.
#[tokio::test]
async fn chb_3_user_activation_requires_the_existing_delegated_journal() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("missing-user-history");
    let collaboration_path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner
        .handoff(CollaborationAttempt {
            id: item("handoff-missing-user-history"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: named_delegation("one"),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("cold Handoff");
    owner.begin_shutdown().await.expect("begin first shutdown");
    owner
        .finish_shutdown()
        .await
        .expect("finish first shutdown");

    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("open delegated directory");
    let missing_path = children
        .path_for(&endpoint("one").conversation)
        .expect("missing child path");
    assert!(!missing_path.exists());
    let writer = CollaborationWriter::spawn(
        CollaborationFile::open(collaboration_path).expect("reopen collaboration"),
    )
    .expect("reopened writer");
    let mut reopened = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    reopened
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main ingress");
    reopened
        .bind_child_factory(crate::DelegatedChildFactory::synthetic(
            children,
            workspace.catalog(),
            FakeDriver::new([]),
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind child factory");
    let target = reopened
        .register_collaboration_targets()
        .await
        .expect("register target")
        .into_iter()
        .find(|target| target.worker() == &endpoint("one"))
        .expect("first target")
        .user_input_target();
    assert!(matches!(
        reopened
            .activate_user_target(&target)
            .await
            .expect_err("missing User history fails closed"),
        crate::UserInputRefusal::Activation(_)
    ));
    assert!(!missing_path.exists());
    reopened
        .begin_shutdown()
        .await
        .expect("begin reopened shutdown");
    reopened
        .finish_shutdown()
        .await
        .expect("finish reopened shutdown");
}

/// COL-3/CHB-2/CHB-3: reopen stays passive until explicit User activation, then keeps history.
#[tokio::test]
async fn col_3_reopened_user_control_requires_explicit_activation_and_preserves_history() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("reopened-user-input");
    let collaboration_path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let initial_driver = FakeDriver::new([Script::Events(vec![
        text_delta("first answer"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let (runtime, _) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "one",
        Arc::clone(&initial_driver),
    )
    .await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main ingress");
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial announcement");
    let targets = owner
        .register_collaboration_targets()
        .await
        .expect("register targets");
    let stale_target = user_target(&targets, "one");
    owner
        .handoff(CollaborationAttempt {
            id: item("handoff-before-reopen"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: named_delegation("one"),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("handoff before reopen");
    let stale_ticket = owner
        .activate_user_target(&stale_target)
        .await
        .expect("ticket");
    owner
        .submit_user_input(UserInputRequest::new(
            stale_ticket.clone(),
            Input::Submitted {
                text: "first user turn".into(),
            },
            None,
        ))
        .await
        .expect("first User-controlled turn");
    drive_child_until_idle(&mut owner, &endpoint("one").conversation).await;
    shutdown_owner(&mut owner).await;
    let children = DelegatedConversationDirectory::under(&directory.0)
        .expect("delegated directory after first owner");
    let child_path = children
        .path_for(&endpoint("one").conversation)
        .expect("child path");
    let child_before = std::fs::read(&child_path).expect("child bytes before passive reopen");
    let reopened_driver = FakeDriver::new([Script::Events(vec![
        text_delta("second answer"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let writer = CollaborationWriter::spawn(
        CollaborationFile::open(&collaboration_path).expect("reopen collaboration"),
    )
    .expect("reopened writer");
    let mut reopened = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    reopened
        .bind_main_ingress(endpoint("main"))
        .expect("bind reopened Main ingress");
    reopened
        .bind_child_factory(crate::DelegatedChildFactory::synthetic(
            children,
            workspace.catalog(),
            reopened_driver.clone(),
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind reopened child factory");
    let targets = reopened
        .register_collaboration_targets()
        .await
        .expect("rebuild canonical target");
    let current_target = user_target(&targets, "one");
    assert!(matches!(
        reopened
            .activate_user_target(&stale_target)
            .await
            .expect_err("prior owner target is stale"),
        crate::UserInputRefusal::StaleTarget
    ));
    let stale = reopened
        .submit_user_input(UserInputRequest::new(
            stale_ticket,
            Input::Submitted {
                text: "stale generation".into(),
            },
            None,
        ))
        .await
        .expect_err("prior owner ticket is stale");
    assert!(matches!(
        stale.reason(),
        crate::UserInputRefusal::StaleTarget
    ));
    assert!(reopened_driver.calls().await.is_empty());
    assert_eq!(
        std::fs::read(&child_path).expect("passive child bytes"),
        child_before
    );
    let ticket = reopened
        .activate_user_target(&current_target)
        .await
        .expect("explicit cold activation");
    assert_eq!(ticket.identity().endpoint(), &endpoint("one"));
    assert!(reopened_driver.calls().await.is_empty());
    assert_eq!(
        reopened
            .wake(WakeHint::new(
                ticket.identity().clone(),
                item("reopened-user-wake"),
                TurnId::new("reopened-user-wake").expect("turn"),
            ))
            .expect_err("reopened User control keeps Main wake closed")
            .reason(),
        &WakeRefusal::HandoffPending
    );
    reopened
        .submit_user_input(UserInputRequest::new(
            ticket,
            Input::Submitted {
                text: "second user turn".into(),
            },
            None,
        ))
        .await
        .expect("reopened User-controlled turn");
    let calls = reopened_driver.calls().await;
    assert_eq!(calls.len(), 1);
    for expected in ["first user turn", "second user turn"] {
        assert!(calls[0].request.atoms.iter().any(|atom| {
            matches!(atom.value(), ContextAtomValue::User { text } if text == expected)
        }));
    }
    shutdown_owner(&mut reopened).await;
}

async fn drive_child_until_idle(owner: &mut OwnedCollaboration, conversation: &ConversationId) {
    for _ in 0..32 {
        let update = tokio::time::timeout(Duration::from_secs(5), owner.next_update())
            .await
            .expect("child update deadline")
            .expect("child remains live through idle");
        if update.identity().endpoint().conversation == *conversation
            && matches!(
                update.runtime_update(),
                Some(RuntimeUpdate::Event(envelope))
                    if matches!(
                        envelope.event,
                        plexmaton_core::ConversationEvent::AgentStatusChanged {
                            status: plexmaton_core::AgentStatus::Idle,
                            ..
                        }
                    )
            )
        {
            return;
        }
    }
    panic!("child did not return to idle");
}

fn user_target(
    targets: &[crate::RegisteredCollaborationTarget],
    worker: &str,
) -> crate::UserInputTarget {
    targets
        .iter()
        .find(|target| target.worker() == &endpoint(worker))
        .expect("registered child target")
        .user_input_target()
}

async fn shutdown_owner(owner: &mut OwnedCollaboration) {
    owner.begin_shutdown().await.expect("begin owner shutdown");
    while owner.next_update().await.is_some() {}
    owner.finish_shutdown().await.expect("join owner shutdown");
}
