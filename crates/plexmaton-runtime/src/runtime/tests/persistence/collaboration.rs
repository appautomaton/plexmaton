use std::path::{Path, PathBuf};

use plexmaton_agent::collaboration::{
    CollaborationContext, CollaborationEvent, CollaborationLimits, CollaborationText,
    DelegationController, DelegationRevision, MailEndpoint, Preparation, ResolvedTurnAdmission,
};
use plexmaton_agent::{
    Agent, ApprovalPolicy, ContextAtomValue, HeadRevision, RetryTarget, TurnBudget,
    UndeliveredReason, UnixMillis,
};
use plexmaton_core::{CollaborationId, CollaborationItemId, DelegationId, ReasoningEffort, TurnId};
use plexmaton_session_store::{
    JournalFile, JournalRecovery,
    collaboration::{CollaborationFile, CollaborationStoreError},
};

use super::*;
use crate::ModelChangeRefusal;
use crate::runtime::tests::{finish_active, text_delta};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("plexmaton-inclusion-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir(&path).expect("reserve test directory");
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned fixtures");
    }
}

#[derive(Clone, Copy)]
enum BoundaryGate {
    Inclusion,
    Authorization,
}

struct GatedFile {
    file: JournalFile,
    gate: Arc<Gate>,
    boundary: BoundaryGate,
    fail_after_write: Arc<AtomicBool>,
}

impl JournalStore for GatedFile {
    fn append(&mut self, record: JournalRecord) -> Result<(), StoreError> {
        let boundary = match self.boundary {
            BoundaryGate::Inclusion => matches!(&record, JournalRecord::AppendEntry { entry, .. }
                if matches!(entry.payload, JournalEntryPayload::CollaborationTurnStarted { .. })),
            BoundaryGate::Authorization => {
                matches!(record, JournalRecord::RequestAttemptAuthorized { .. })
            }
        };
        if boundary {
            self.gate.wait();
        }
        self.file
            .append(record)
            .map_err(|failure| failure.into_parts().0)?;
        if boundary && self.fail_after_write.load(Ordering::SeqCst) {
            return Err(StoreError::Io {
                operation: "lost acknowledgement",
                source: std::io::Error::other("injected uncertain reply"),
            });
        }
        Ok(())
    }
}

fn endpoint(runtime: &LiveRuntime) -> MailEndpoint {
    MailEndpoint {
        agent: runtime.agent_id().clone(),
        conversation: runtime.agent.journal().conversation_id().clone(),
    }
}
fn item(name: &str) -> CollaborationItemId {
    CollaborationItemId::new(name).expect("item")
}
fn text(value: &str) -> CollaborationText {
    CollaborationText::new(value).expect("text")
}
fn delegation() -> DelegationId {
    DelegationId::new("task").expect("id")
}
fn collaboration_id() -> CollaborationId {
    CollaborationId::new("collaboration").expect("id")
}
fn update(author: MailEndpoint, expected: u64, task: &str) -> CollaborationEvent {
    CollaborationEvent::TaskUpdated {
        delegation: DelegationId::new("task").expect("id"),
        expected: DelegationRevision(expected),
        author,
        task: text(task),
    }
}
fn tools(directory: &Path) -> NativeToolCatalog {
    NativeToolCatalog::open(
        directory,
        "TEST_KEY",
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .expect("test tool catalog")
}

async fn runner<D: crate::runtime::ModelDriver>(
    directory: &Path,
    name: &str,
    driver: Arc<D>,
    boundary: BoundaryGate,
) -> (LiveRuntime, Arc<Gate>, Arc<AtomicBool>, PathBuf) {
    let session_id = ConversationId::new(format!("session-{name}")).expect("session");
    let path = directory.join(format!("session-{name}.jsonl"));
    let file =
        JournalFile::create(&path, session_id.clone(), UnixMillis::EPOCH).expect("create session");
    let gate = Arc::new(Gate::new());
    let fail = Arc::new(AtomicBool::new(false));
    let store = GatedFile {
        file,
        gate: Arc::clone(&gate),
        boundary,
        fail_after_write: Arc::clone(&fail),
    };
    let clock = Arc::new(crate::runtime::FixedWallClock(UnixMillis::EPOCH));
    let runtime = LiveRuntime::with_delegated_driver_store_and_clock(
        AgentId::new(name).expect("agent"),
        name.into(),
        driver,
        tools(directory),
        ConversationMetadata::new(session_id, UnixMillis::EPOCH),
        Box::new(store),
        clock,
    )
    .await
    .expect("construct runner");
    (runtime, gate, fail, path)
}

fn collaboration(directory: &Path, a: MailEndpoint, b: MailEndpoint) -> CollaborationFile {
    let mut file = CollaborationFile::create(
        directory.join("private/collaboration.jsonl"),
        collaboration_id(),
        CollaborationLimits::default(),
    )
    .expect("create collaboration");
    file.admit(
        item("delegate"),
        CollaborationEvent::DelegationCreated {
            delegation: DelegationId::new("task").expect("id"),
            delegator: a,
            worker: b,
            task: text("Inspect the parser"),
        },
    )
    .expect("delegate");
    file
}

fn admit(
    file: &mut CollaborationFile,
    runtime: &LiveRuntime,
    name: &str,
) -> Arc<ResolvedTurnAdmission> {
    let (boundary, previous) = runtime
        .collaboration_boundary(TurnId::new(name).expect("turn"))
        .expect("idle boundary");
    let Preparation::Append(record) = file
        .ledger()
        .prepare_turn(item(name), boundary, previous)
        .expect("prepare")
    else {
        panic!("fresh turn")
    };
    file.admit(record.id, record.event)
        .expect("append admission");
    file.ledger()
        .resolve_turn(
            &file
                .ledger()
                .item_reference(&item(name))
                .expect("reference"),
        )
        .expect("resolve")
}

fn dummy_child() -> MailEndpoint {
    MailEndpoint {
        conversation: ConversationId::new("session-b").expect("id"),
        agent: AgentId::new("b").expect("id"),
    }
}

fn dummy_main() -> MailEndpoint {
    MailEndpoint {
        conversation: ConversationId::new("session-main").expect("id"),
        agent: AgentId::new("main").expect("id"),
    }
}

fn handoff(author: MailEndpoint, expected: u64) -> CollaborationEvent {
    CollaborationEvent::HandoffCompleted {
        delegation: delegation(),
        expected: DelegationRevision(expected),
        author,
    }
}

fn assert_foreign_control_refused(
    directory: &Path,
    runtime: &mut LiveRuntime,
    main: &MailEndpoint,
) {
    let mut foreign = CollaborationFile::create(
        directory.join("private/foreign-collaboration.jsonl"),
        CollaborationId::new("foreign-collaboration").expect("id"),
        CollaborationLimits::default(),
    )
    .expect("create foreign collaboration");
    foreign
        .admit(
            item("foreign-delegate"),
            CollaborationEvent::DelegationCreated {
                delegation: delegation(),
                delegator: main.clone(),
                worker: endpoint(runtime),
                task: text("Wrong collaboration"),
            },
        )
        .expect("create foreign delegation");
    assert!(matches!(
        runtime.attach_delegated_control(
            &collaboration_id(),
            &delegation(),
            foreign
                .delegated_control(&delegation())
                .expect("foreign child control"),
        ),
        Err(RuntimeError::DelegatedControlMismatch)
    ));
}

/// COL-3: a delegated runtime refuses direct input until its exact durable control is attached.
#[tokio::test]
async fn col_3_unbound_delegated_runtime_fails_closed() {
    let directory = Directory::new();
    let driver = FakeDriver::new([]);
    let (mut runtime, _, _, _) = runner(
        &directory.0,
        "b",
        Arc::clone(&driver),
        BoundaryGate::Inclusion,
    )
    .await;
    let before = runtime.agent.journal().clone();
    let runtime_agent = runtime.agent_id().clone();

    assert!(matches!(
        runtime.delegation_controller(),
        Err(RuntimeError::DelegatedControlUnavailable)
    ));
    let refused = runtime
        .submit(
            runtime_agent.clone(),
            Input::Submitted {
                text: "must not fail open".into(),
            },
        )
        .await
        .expect("typed unavailable-control refusal");
    assert!(refused.undelivered.iter().any(|input| {
        input.text == "must not fail open" && input.reason == UndeliveredReason::ControlUnavailable
    }));
    let refused = runtime
        .submit_skill(
            runtime_agent.clone(),
            Input::Submitted {
                text: "skill must wait".into(),
            },
            "review".into(),
        )
        .await
        .expect("typed unavailable-control skill refusal");
    assert!(refused.undelivered.iter().any(|input| {
        input.text == "skill must wait"
            && input.skill.as_deref() == Some("review")
            && input.reason == UndeliveredReason::ControlUnavailable
    }));
    let retry_target = RetryTarget {
        turn_id: TurnId::new("missing-retry").expect("turn"),
        head_revision: HeadRevision::new(0),
    };
    assert!(matches!(
        runtime.retry(retry_target, None).await,
        Err(RuntimeError::DelegatedControlUnavailable)
    ));
    assert_eq!(
        runtime.set_reasoning_effort(&runtime_agent, ReasoningEffort::Max),
        Err(ModelChangeRefusal::ControlUnavailable)
    );
    assert_eq!(runtime.agent.journal(), &before);
    assert!(driver.calls().await.is_empty());
    runtime.shutdown().await.expect("shutdown unbound runtime");
}

/// COL-3: permission refresh is internal control input and cannot break a Main-controlled session.
#[tokio::test]
async fn col_3_permission_refresh_remains_available_under_main_control() {
    let directory = Directory::new();
    let driver = FakeDriver::new([]);
    let (mut runtime, _, _, _) = runner(
        &directory.0,
        "b",
        Arc::clone(&driver),
        BoundaryGate::Inclusion,
    )
    .await;
    let main = dummy_main();
    let collaboration = collaboration(&directory.0, main, endpoint(&runtime));
    runtime
        .attach_delegated_control(
            &collaboration_id(),
            &delegation(),
            collaboration
                .delegated_control(&delegation())
                .expect("child control"),
        )
        .expect("attach control");
    let before = runtime.agent.journal().clone();

    let report = runtime
        .permissions_changed()
        .await
        .expect("permission refresh is not direct user input");
    assert!(report.is_empty());
    assert_eq!(runtime.agent.journal(), &before);
    assert_eq!(
        runtime.delegation_controller().expect("controller"),
        Some(DelegationController::Main)
    );
    assert!(driver.calls().await.is_empty());
    runtime.shutdown().await.expect("shutdown");
}

/// COL-3: Main-owned child input refuses exact user text and settings until durable handoff.
#[tokio::test]
async fn col_3_main_control_gates_direct_input_until_handoff() {
    let directory = Directory::new();
    let driver = FakeDriver::new([Script::Events(vec![
        text_delta("user continuation"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let (mut runtime, _, _, _) = runner(
        &directory.0,
        "b",
        Arc::clone(&driver),
        BoundaryGate::Inclusion,
    )
    .await;
    let main = dummy_main();
    let mut collaboration = collaboration(&directory.0, main.clone(), endpoint(&runtime));
    assert_foreign_control_refused(&directory.0, &mut runtime, &main);
    runtime
        .attach_delegated_control(
            &collaboration_id(),
            &delegation(),
            collaboration
                .delegated_control(&delegation())
                .expect("child control"),
        )
        .expect("attach exact child control");
    assert_eq!(
        runtime.delegation_controller().expect("controller"),
        Some(DelegationController::Main)
    );
    let before = runtime.agent.journal().clone();
    let runtime_agent = runtime.agent_id().clone();

    let refused = runtime
        .submit(
            runtime.agent_id().clone(),
            Input::Submitted {
                text: "user must wait".into(),
            },
        )
        .await
        .expect("typed input refusal");
    assert!(refused.undelivered.iter().any(|input| {
        input.text == "user must wait" && input.reason == UndeliveredReason::ControlledByMain
    }));
    let refused = runtime
        .submit_skill(
            runtime.agent_id().clone(),
            Input::Steered {
                text: "steer with skill".into(),
            },
            "review".into(),
        )
        .await
        .expect("typed skill refusal");
    assert!(refused.undelivered.iter().any(|input| {
        input.text == "steer with skill"
            && input.skill.as_deref() == Some("review")
            && input.reason == UndeliveredReason::ControlledByMain
    }));
    let retry_target = RetryTarget {
        turn_id: TurnId::new("missing-retry").expect("turn"),
        head_revision: HeadRevision::new(0),
    };
    let refused = runtime
        .retry(retry_target.clone(), Some("edited retry".into()))
        .await
        .expect("edited retry refusal");
    assert!(refused.undelivered.iter().any(|input| {
        input.text == "edited retry" && input.reason == UndeliveredReason::ControlledByMain
    }));
    assert!(matches!(
        runtime.retry(retry_target, None).await,
        Err(RuntimeError::ControlledByMain)
    ));
    assert_eq!(
        runtime.set_reasoning_effort(&runtime_agent, ReasoningEffort::Max),
        Err(ModelChangeRefusal::ControlledByMain)
    );
    assert_eq!(runtime.agent.journal(), &before);
    assert!(driver.calls().await.is_empty());

    collaboration
        .admit(item("handoff"), handoff(main, 0))
        .expect("durable handoff");
    assert_eq!(
        runtime.delegation_controller().expect("controller"),
        Some(DelegationController::User)
    );
    runtime
        .submit(
            runtime.agent_id().clone(),
            Input::Submitted {
                text: "continue after handoff".into(),
            },
        )
        .await
        .expect("user input after handoff");
    finish_active(&mut runtime).await;
    let calls = driver.calls().await;
    assert_eq!(calls.len(), 1);
    assert!(calls[0].request.atoms.iter().any(
        |atom| matches!(atom.value(), ContextAtomValue::User { text } if text == "continue after handoff")
    ));
    assert!(
        runtime
            .agent
            .journal()
            .records()
            .iter()
            .any(|record| matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(&entry.payload, JournalEntryPayload::TurnStarted { text, .. }
                        if text == "continue after handoff")
            ))
    );
    runtime.shutdown().await.expect("shutdown");
}

/// COL-3/CIN-4: a permit spans acknowledgement and each later Main turn needs a fresh admission.
#[tokio::test]
async fn col_3_permit_spans_barriers_and_two_main_rounds() {
    for boundary in [BoundaryGate::Inclusion, BoundaryGate::Authorization] {
        let directory = Directory::new();
        let driver = FakeDriver::new([
            Script::Events(vec![
                text_delta("first"),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
            Script::Events(vec![
                text_delta("second"),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ]),
        ]);
        let (mut runtime, gate, _, _) =
            runner(&directory.0, "b", Arc::clone(&driver), boundary).await;
        let _release_gate = ReleaseGateOnDrop(&gate);
        let main = dummy_main();
        let mut collaboration = collaboration(&directory.0, main.clone(), endpoint(&runtime));
        let control = collaboration
            .delegated_control(&delegation())
            .expect("child control");
        runtime
            .attach_delegated_control(&collaboration_id(), &delegation(), control.clone())
            .expect("attach control");

        let reservation = control.reserve_execution().expect("reserve first input");
        let first = admit(&mut collaboration, &runtime, "first-main-turn");
        let ticket = collaboration
            .execution_ticket(&delegation(), &first)
            .expect("first ticket");
        gate.arm();
        {
            let entered = gate.entered.notified();
            let start = runtime.start_delegated_turn(reservation, ticket, Arc::clone(&first));
            tokio::pin!(start);
            tokio::select! {
                result = &mut start => panic!("started before durability acknowledgement: {result:?}"),
                () = entered => {}
            }
            assert!(driver.calls().await.is_empty());
        }
        assert!(runtime.has_active_work());
        assert!(matches!(
            collaboration
                .admit(item("early-handoff"), handoff(main.clone(), 0))
                .expect_err("pending inclusion retains permit")
                .error(),
            CollaborationStoreError::ControlNotQuiescent
        ));
        let refused = runtime
            .submit(
                runtime.agent_id().clone(),
                Input::Submitted {
                    text: "still Main controlled".into(),
                },
            )
            .await
            .expect("input refusal while inclusion is pending");
        assert_eq!(
            refused.undelivered[0].reason,
            UndeliveredReason::ControlledByMain
        );
        gate.release();
        finish_active(&mut runtime).await;
        assert!(!runtime.has_active_work());

        collaboration
            .admit(
                item("second-update"),
                update(main.clone(), 0, "Inspect the follow-up evidence"),
            )
            .expect("new Main source for the second turn");
        let reservation = control.reserve_execution().expect("reserve second input");
        let second = admit(&mut collaboration, &runtime, "second-main-turn");
        assert_ne!(first.reference(), second.reference());
        let ticket = collaboration
            .execution_ticket(&delegation(), &second)
            .expect("second ticket");
        runtime
            .start_delegated_turn(reservation, ticket, Arc::clone(&second))
            .await
            .expect("second Main turn");
        finish_active(&mut runtime).await;
        let calls = driver.calls().await;
        assert_eq!(calls.len(), 2);
        assert!(calls[1].request.atoms.iter().any(|atom| matches!(
            atom.value(),
            ContextAtomValue::Collaboration(CollaborationContext::Resolved(value))
                if value == &second
        )));
        collaboration
            .admit(item("handoff"), handoff(main, 1))
            .expect("quiescent Handoff after both turns");
        runtime.shutdown().await.expect("shutdown");
    }
}

/// COL-3/LIVE-3: stop remains available and the joined child releases its execution permit.
#[tokio::test]
async fn col_3_stop_joins_main_owned_child_before_handoff() {
    let directory = Directory::new();
    let started = Arc::new(Notify::new());
    let finished = Arc::new(AtomicBool::new(false));
    let driver = FakeDriver::new([Script::WaitForCancellation {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    }]);
    let (mut runtime, _, _, _) = runner(
        &directory.0,
        "b",
        Arc::clone(&driver),
        BoundaryGate::Inclusion,
    )
    .await;
    let main = dummy_main();
    let mut collaboration = collaboration(&directory.0, main.clone(), endpoint(&runtime));
    let control = collaboration
        .delegated_control(&delegation())
        .expect("child control");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control.clone())
        .expect("attach control");
    let reservation = control.reserve_execution().expect("reserve Main input");
    let admitted = admit(&mut collaboration, &runtime, "running-child");
    let ticket = collaboration
        .execution_ticket(&delegation(), &admitted)
        .expect("ticket");
    runtime
        .start_delegated_turn(reservation, ticket, admitted)
        .await
        .expect("start child");
    {
        let started = started.notified();
        let drive = async {
            loop {
                runtime.next_update().await.expect("child progress");
            }
        };
        tokio::pin!(drive);
        tokio::select! {
            () = started => {},
            () = &mut drive => panic!("child stopped before cancellation")
        }
    }
    assert!(runtime.has_active_model());
    assert!(matches!(
        collaboration
            .admit(item("active-handoff"), handoff(main.clone(), 0))
            .expect_err("active model retains permit")
            .error(),
        CollaborationStoreError::ControlNotQuiescent
    ));
    let refused = runtime
        .submit(
            runtime.agent_id().clone(),
            Input::Submitted {
                text: "cannot steer active child".into(),
            },
        )
        .await
        .expect("active input refusal");
    assert!(refused.undelivered.iter().any(|input| {
        input.text == "cannot steer active child"
            && input.reason == UndeliveredReason::ControlledByMain
    }));
    assert!(runtime.has_active_model());
    runtime
        .submit(runtime.agent_id().clone(), Input::Interrupted)
        .await
        .expect("stop is available");
    assert!(finished.load(Ordering::SeqCst));
    assert!(!runtime.has_active_work());
    collaboration
        .admit(item("handoff"), handoff(main, 0))
        .expect("handoff after joined stop");
    runtime.shutdown().await.expect("shutdown");
}

/// COL-3/LIVE-3: shutdown remains available and joins a Main-controlled child before exit.
#[tokio::test]
async fn col_3_shutdown_joins_main_owned_child_and_releases_authority() {
    let directory = Directory::new();
    let started = Arc::new(Notify::new());
    let finished = Arc::new(AtomicBool::new(false));
    let driver = FakeDriver::new([Script::WaitForCancellation {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    }]);
    let (mut runtime, _, _, _) = runner(
        &directory.0,
        "b",
        Arc::clone(&driver),
        BoundaryGate::Inclusion,
    )
    .await;
    let main = dummy_main();
    let mut collaboration = collaboration(&directory.0, main.clone(), endpoint(&runtime));
    let control = collaboration
        .delegated_control(&delegation())
        .expect("child control");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control.clone())
        .expect("attach control");
    let reservation = control.reserve_execution().expect("reserve Main input");
    let admitted = admit(&mut collaboration, &runtime, "shutdown-child");
    let ticket = collaboration
        .execution_ticket(&delegation(), &admitted)
        .expect("ticket");
    runtime
        .start_delegated_turn(reservation, ticket, admitted)
        .await
        .expect("start child");
    {
        let started = started.notified();
        let drive = async {
            loop {
                runtime.next_update().await.expect("child progress");
            }
        };
        tokio::pin!(drive);
        tokio::select! {
            () = started => {},
            () = &mut drive => panic!("child stopped before shutdown")
        }
    }
    runtime
        .shutdown()
        .await
        .expect("shutdown remains available");
    assert!(finished.load(Ordering::SeqCst));
    assert!(!runtime.has_active_work());
    collaboration
        .admit(item("handoff"), handoff(main, 0))
        .expect("joined shutdown released authority");
}

/// COL-3/LIVE-3: provider failure during stop cannot retain a quiescent execution permit.
#[tokio::test]
async fn col_3_failed_provider_join_releases_permit_after_cleanup() {
    let directory = Directory::new();
    let started = Arc::new(Notify::new());
    let driver = FakeDriver::new([Script::PanicAfterCancellation {
        started: Arc::clone(&started),
    }]);
    let (mut runtime, _, _, _) = runner(
        &directory.0,
        "b",
        Arc::clone(&driver),
        BoundaryGate::Inclusion,
    )
    .await;
    let main = dummy_main();
    let mut collaboration = collaboration(&directory.0, main.clone(), endpoint(&runtime));
    let control = collaboration
        .delegated_control(&delegation())
        .expect("child control");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control.clone())
        .expect("attach control");
    let reservation = control.reserve_execution().expect("reserve Main input");
    let admitted = admit(&mut collaboration, &runtime, "failing-child");
    let ticket = collaboration
        .execution_ticket(&delegation(), &admitted)
        .expect("ticket");
    runtime
        .start_delegated_turn(reservation, ticket, admitted)
        .await
        .expect("start child");
    {
        let started = started.notified();
        let drive = async {
            loop {
                runtime.next_update().await.expect("child progress");
            }
        };
        tokio::pin!(drive);
        tokio::select! {
            () = started => {},
            () = &mut drive => panic!("child stopped before cancellation")
        }
    }
    assert!(runtime.has_active_model());
    assert!(matches!(
        runtime
            .submit(runtime.agent_id().clone(), Input::Interrupted)
            .await,
        Err(RuntimeError::ProviderFutureFailed(_))
    ));
    assert!(!runtime.has_active_work());
    collaboration
        .admit(item("handoff"), handoff(main, 0))
        .expect("failed join released quiescent permit");
    runtime
        .shutdown()
        .await
        .expect("shutdown after failed join");
}

/// COL-3/CIN-4: a definite session-writer refusal releases bound authority after owned cleanup.
#[tokio::test]
async fn col_3_start_failure_releases_permit_after_cleanup() {
    let directory = Directory::new();
    let driver = FakeDriver::new([]);
    let (mut runtime, _, _, _) = runner(
        &directory.0,
        "b",
        Arc::clone(&driver),
        BoundaryGate::Inclusion,
    )
    .await;
    let main = dummy_main();
    let mut collaboration = collaboration(&directory.0, main.clone(), endpoint(&runtime));
    let control = collaboration
        .delegated_control(&delegation())
        .expect("child control");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control.clone())
        .expect("attach control");
    runtime
        .journal
        .as_mut()
        .expect("session writer")
        .shutdown()
        .await
        .expect("stop writer");
    let reservation = control.reserve_execution().expect("reserve Main input");
    let admitted = admit(&mut collaboration, &runtime, "writer-stopped");
    let ticket = collaboration
        .execution_ticket(&delegation(), &admitted)
        .expect("ticket");
    assert!(matches!(
        runtime
            .start_delegated_turn(reservation, ticket, admitted)
            .await,
        Err(RuntimeError::JournalWriterUnavailable)
    ));
    assert!(!runtime.has_active_work());
    assert!(driver.calls().await.is_empty());
    collaboration
        .admit(item("handoff"), handoff(main, 0))
        .expect("definite failure released permit");
}

/// COL-3/LIVE-3: runtime Drop joins a pending session append before releasing child authority.
#[tokio::test]
async fn col_3_runtime_drop_joins_session_writer_before_permit_release() {
    let directory = Directory::new();
    let driver = FakeDriver::new([]);
    let (mut runtime, gate, _, _) =
        runner(&directory.0, "b", driver, BoundaryGate::Inclusion).await;
    let _release_gate = ReleaseGateOnDrop(&gate);
    let main = dummy_main();
    let mut collaboration = collaboration(&directory.0, main.clone(), endpoint(&runtime));
    let control = collaboration
        .delegated_control(&delegation())
        .expect("child control");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control.clone())
        .expect("attach control");
    let reservation = control.reserve_execution().expect("reserve Main input");
    let admitted = admit(&mut collaboration, &runtime, "drop-pending");
    let ticket = collaboration
        .execution_ticket(&delegation(), &admitted)
        .expect("ticket");
    gate.arm();
    {
        let entered = gate.entered.notified();
        let start = runtime.start_delegated_turn(reservation, ticket, admitted);
        tokio::pin!(start);
        tokio::select! {
            result = &mut start => panic!("started before inclusion acknowledgement: {result:?}"),
            () = entered => {}
        }
    }

    let (drop_entered_tx, drop_entered_rx) = std::sync::mpsc::channel();
    runtime
        .journal
        .as_mut()
        .expect("session writer")
        .set_drop_probe(drop_entered_tx);
    let dropper = std::thread::spawn(move || drop(runtime));
    drop_entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("runtime Drop reached the journal join before the permit field");
    assert!(matches!(
        collaboration
            .admit(item("early-drop-handoff"), handoff(main.clone(), 0))
            .expect_err("runtime Drop retains permit while writer joins")
            .error(),
        CollaborationStoreError::ControlNotQuiescent
    ));
    gate.release();
    dropper.join().expect("runtime Drop joined writer");
    collaboration
        .admit(item("handoff"), handoff(main, 0))
        .expect("joined runtime released permit");
}

/// CIN-4: each real session write boundary holds the scripted driver at zero calls.
#[tokio::test]
async fn cin_4_inclusion_and_request_authorization_each_gate_dispatch() {
    for boundary in [BoundaryGate::Inclusion, BoundaryGate::Authorization] {
        let directory = Directory::new();
        let driver = FakeDriver::new([Script::Events(vec![
            text_delta("done"),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ])]);
        let (mut runtime, gate, _, path) =
            runner(&directory.0, "b", Arc::clone(&driver), boundary).await;
        let _release = ReleaseGateOnDrop(&gate);
        let main = dummy_main();
        let mut collaboration = collaboration(&directory.0, main.clone(), endpoint(&runtime));
        let control = collaboration
            .delegated_control(&delegation())
            .expect("child control");
        runtime
            .attach_delegated_control(&collaboration_id(), &delegation(), control.clone())
            .expect("attach control");
        collaboration
            .admit(item("update"), update(main, 0, "Inspect tests only"))
            .expect("main task update");
        let reservation = control.reserve_execution().expect("reserve Main input");
        let admitted = admit(&mut collaboration, &runtime, "turn");
        let ticket = collaboration
            .execution_ticket(&delegation(), &admitted)
            .expect("ticket");
        gate.arm();
        {
            let entered = gate.entered.notified();
            let start = runtime.start_delegated_turn(reservation, ticket, Arc::clone(&admitted));
            tokio::pin!(start);
            tokio::select! {
                result = &mut start => panic!("started before acknowledgement: {result:?}"),
                () = entered => {}
            }
            assert!(driver.calls().await.is_empty());
            let bytes = std::fs::read_to_string(&path).expect("read session while held");
            assert!(
                !bytes.contains("Inspect tests only"),
                "body belongs only to collaboration log"
            );
            assert_eq!(
                bytes.contains("collaboration_turn_started"),
                matches!(boundary, BoundaryGate::Authorization)
            );
            gate.release();
            start.await.expect("acknowledged start");
        }
        let calls = driver.calls().await;
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0]
                .request
                .atoms
                .iter()
                .all(|atom| !matches!(atom.value(), ContextAtomValue::User { .. }))
        );
        assert!(
            matches!(calls[0].request.atoms[0].value(), ContextAtomValue::Collaboration(CollaborationContext::Resolved(value))
            if value == &admitted && value.items().last().expect("update").delegation_revision == Some(DelegationRevision(1)))
        );
        finish_active(&mut runtime).await;
        runtime.shutdown().await.expect("joined shutdown");
    }
}

/// CIN-4: dropping a waiting start future leaves its exact pending inclusion owned and resumable.
#[tokio::test]
async fn cin_4_cancelled_start_retains_inclusion_until_acknowledgement() {
    let directory = Directory::new();
    let driver = FakeDriver::new([Script::Events(vec![
        text_delta("done"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let (mut runtime, gate, _, _) = runner(
        &directory.0,
        "b",
        Arc::clone(&driver),
        BoundaryGate::Inclusion,
    )
    .await;
    let _release = ReleaseGateOnDrop(&gate);
    let main = dummy_main();
    let mut collaboration = collaboration(&directory.0, main.clone(), endpoint(&runtime));
    let control = collaboration
        .delegated_control(&delegation())
        .expect("child control");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control.clone())
        .expect("attach control");
    let reservation = control.reserve_execution().expect("reserve Main input");
    let admitted = admit(&mut collaboration, &runtime, "turn");
    let ticket = collaboration
        .execution_ticket(&delegation(), &admitted)
        .expect("ticket");
    gate.arm();
    {
        let entered = gate.entered.notified();
        let start = runtime.start_delegated_turn(reservation, ticket, admitted);
        tokio::pin!(start);
        tokio::select! { result = &mut start => panic!("unexpected completion {result:?}"), () = entered => {} }
    }
    assert!(driver.calls().await.is_empty());
    assert!(matches!(
        collaboration
            .admit(item("cancelled-handoff"), handoff(main, 0))
            .expect_err("cancelled wait retains permit")
            .error(),
        CollaborationStoreError::ControlNotQuiescent
    ));
    assert!(
        runtime
            .collaboration_boundary(TurnId::new("other").expect("id"))
            .is_err()
    );
    gate.release();
    finish_active(&mut runtime).await;
    assert_eq!(driver.calls().await.len(), 1);
    runtime.shutdown().await.expect("shutdown");
}

/// CIN-2/CIN-4: a lost inclusion acknowledgement requires reopen; recovery never redispatches.
#[tokio::test]
async fn cin_4_uncertain_inclusion_reopens_without_redispatch() {
    let directory = Directory::new();
    let driver = FakeDriver::new([]);
    let (mut runtime, _, fail, path) = runner(
        &directory.0,
        "b",
        Arc::clone(&driver),
        BoundaryGate::Inclusion,
    )
    .await;
    let mut collaboration = collaboration(&directory.0, dummy_main(), endpoint(&runtime));
    let control = collaboration
        .delegated_control(&delegation())
        .expect("child control");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control.clone())
        .expect("attach control");
    let reservation = control.reserve_execution().expect("reserve Main input");
    let admitted = admit(&mut collaboration, &runtime, "turn");
    let ticket = collaboration
        .execution_ticket(&delegation(), &admitted)
        .expect("ticket");
    let collaboration_path = collaboration.path().to_path_buf();
    fail.store(true, Ordering::SeqCst);
    assert!(
        runtime
            .start_delegated_turn(reservation, ticket, Arc::clone(&admitted))
            .await
            .is_err()
    );
    assert!(driver.calls().await.is_empty());
    assert!(
        runtime
            .collaboration_boundary(TurnId::new("later").expect("id"))
            .is_err()
    );
    drop(runtime);
    drop(collaboration);
    let collaboration = CollaborationFile::open(collaboration_path).expect("reopen collaboration");
    assert!(matches!(
        collaboration.execution_ticket(&delegation(), &admitted),
        Err(CollaborationStoreError::ExecutionAlreadyIssued)
    ));
    let file = JournalFile::open(&path).expect("reopen included reference");
    let agent = Agent::from_journal(
        AgentId::new("b").expect("id"),
        file.journal().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .expect("restore agent");
    let recovery_driver = FakeDriver::new([]);
    let (mut restored, recovery) = LiveRuntime::with_resumed_delegated_driver_and_store(
        AgentId::new("b").expect("id"),
        agent,
        recovery_driver.clone(),
        tools(&directory.0),
        Box::new(file),
        JournalRecovery::Clean,
        Arc::new(crate::runtime::FixedWallClock(UnixMillis::EPOCH)),
    )
    .await
    .expect("recover runtime");
    assert!(recovery.interrupted_turn);
    assert!(recovery_driver.calls().await.is_empty());
    restored
        .attach_delegated_control(
            &collaboration_id(),
            &delegation(),
            collaboration
                .delegated_control(&delegation())
                .expect("reopened child control"),
        )
        .expect("reattach control after reopen");
    restored
        .restore_collaboration_context(collaboration.ledger())
        .expect("restore references only");
    assert!(recovery_driver.calls().await.is_empty());
    assert_eq!(
        restored
            .collaboration_boundary(TurnId::new("later").expect("id"))
            .expect("new boundary")
            .1,
        Some(admitted.reference().clone())
    );
    restored.shutdown().await.expect("shutdown recovered owner");
}

/// CIN-4/LIVE-3: two independent runtime owners retain separate contexts and joined shutdown.
#[tokio::test]
async fn cin_4_two_scripted_runtimes_progress_and_stop_independently() {
    let directory = Directory::new();
    let started = Arc::new(Notify::new());
    let finished = Arc::new(AtomicBool::new(false));
    let driver_b = FakeDriver::new([Script::WaitForCancellation {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    }]);
    let driver_a = FakeDriver::new([Script::Events(vec![
        text_delta("parent done"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let (mut a, _, _, _) = runner(
        &directory.0,
        "a",
        Arc::clone(&driver_a),
        BoundaryGate::Inclusion,
    )
    .await;
    let (mut b, _, _, _) = runner(
        &directory.0,
        "b",
        Arc::clone(&driver_b),
        BoundaryGate::Inclusion,
    )
    .await;
    let mut collaboration = collaboration(&directory.0, endpoint(&a), endpoint(&b));
    let child_turn = admit(&mut collaboration, &b, "child");
    b.start_collaboration_turn(child_turn)
        .await
        .expect("child start");
    {
        let started = started.notified();
        let drive = async {
            loop {
                b.next_update().await.expect("child progress");
            }
        };
        tokio::pin!(drive);
        tokio::select! { () = started => {}, () = &mut drive => panic!("child stopped unexpectedly") }
    }
    let parent_turn = admit(&mut collaboration, &a, "parent");
    a.start_collaboration_turn(parent_turn)
        .await
        .expect("parent start");
    finish_active(&mut a).await;
    assert!(!a.has_active_work());
    assert!(b.has_active_model());
    assert_eq!(
        driver_a.calls().await[0].request.session_id,
        endpoint(&a).conversation
    );
    assert_eq!(
        driver_b.calls().await[0].request.session_id,
        endpoint(&b).conversation
    );
    b.submit(b.agent_id().clone(), Input::Interrupted)
        .await
        .expect("stop child");
    assert!(finished.load(Ordering::SeqCst));
    a.shutdown().await.expect("join parent");
    b.shutdown().await.expect("join child");
    assert!(!a.has_active_work() && !b.has_active_work());
}

/// CIN-1/CIN-2: reopening between the two logs preserves eligibility, including a later update.
#[tokio::test]
async fn cin_2_reopen_between_logs_keeps_unincluded_items_pending() {
    let directory = Directory::new();
    let driver = FakeDriver::new([]);
    let (runtime, _, _, session_path) =
        runner(&directory.0, "a", driver, BoundaryGate::Inclusion).await;
    let mut file = collaboration(&directory.0, endpoint(&runtime), dummy_child());
    let abandoned = admit(&mut file, &runtime, "abandoned");
    file.admit(
        item("update"),
        update(endpoint(&runtime), 0, "Read tests only"),
    )
    .expect("update after unused admission");
    let collaboration_path = file.path().to_path_buf();
    drop(file);
    drop(runtime);
    let mut file = CollaborationFile::open(collaboration_path).expect("reopen collaboration");
    let session = JournalFile::open(&session_path).expect("reopen session");
    let restored_agent = Agent::from_journal(
        AgentId::new("a").expect("id"),
        session.journal().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .expect("agent");
    let driver = FakeDriver::new([Script::Events(vec![
        text_delta("done"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let (mut runtime, recovery) = LiveRuntime::with_resumed_driver_and_store(
        AgentId::new("a").expect("id"),
        restored_agent,
        driver.clone(),
        tools(&directory.0),
        Box::new(session),
        JournalRecovery::Clean,
        Arc::new(crate::runtime::FixedWallClock(UnixMillis::EPOCH)),
    )
    .await
    .expect("restore runner");
    assert!(!recovery.interrupted_turn, "no session turn was recorded");
    assert!(driver.calls().await.is_empty());
    assert_eq!(
        runtime
            .collaboration_boundary(TurnId::new("explicit").expect("id"))
            .expect("boundary")
            .1,
        None
    );
    let explicit = admit(&mut file, &runtime, "explicit");
    assert_eq!(abandoned.items().len(), 1);
    assert_eq!(explicit.items().len(), 2);
    assert_eq!(
        explicit.items()[1].delegation_revision,
        Some(DelegationRevision(1))
    );
    runtime
        .start_collaboration_turn(explicit)
        .await
        .expect("explicit start");
    assert_eq!(driver.calls().await.len(), 1);
    finish_active(&mut runtime).await;
    runtime.shutdown().await.expect("shutdown");
}

/// CIN-3/CIN-4: unresolved historical references refuse explicit admission and settle other
/// preparation failures without leaving a running turn or an authorized request behind.
#[tokio::test]
async fn cin_4_unresolved_history_never_strands_an_authorized_step() {
    let directory = Directory::new();
    let initial_driver = FakeDriver::new([Script::Events(vec![
        text_delta("done"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let (mut runtime, _, _, path) =
        runner(&directory.0, "a", initial_driver, BoundaryGate::Inclusion).await;
    let mut collaboration = collaboration(&directory.0, endpoint(&runtime), dummy_child());
    let source = admit(&mut collaboration, &runtime, "initial");
    runtime
        .start_collaboration_turn(source)
        .await
        .expect("start");
    finish_active(&mut runtime).await;
    runtime.shutdown().await.expect("shutdown");
    drop(runtime);
    let session = JournalFile::open(&path).expect("reopen");
    let agent = Agent::from_journal(
        AgentId::new("a").expect("id"),
        session.journal().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .expect("agent");
    let driver = FakeDriver::new([]);
    let (mut runtime, _) = LiveRuntime::with_resumed_driver_and_store(
        AgentId::new("a").expect("id"),
        agent,
        driver.clone(),
        tools(&directory.0),
        Box::new(session),
        JournalRecovery::Clean,
        Arc::new(crate::runtime::FixedWallClock(UnixMillis::EPOCH)),
    )
    .await
    .expect("restore without materialization");
    collaboration
        .admit(
            item("update"),
            update(endpoint(&runtime), 0, "A later task"),
        )
        .expect("update");
    let source = admit(&mut collaboration, &runtime, "later");
    let before = runtime.agent.journal().clone();
    let error = runtime
        .start_collaboration_turn(source)
        .await
        .expect_err("historical context missing");
    assert!(matches!(
        error,
        RuntimeError::Collaboration(
            plexmaton_agent::collaboration::CollaborationError::UnresolvedContext
        )
    ));
    assert_eq!(runtime.agent.journal(), &before);
    let attempts = runtime.agent.journal().request_attempts().count();
    runtime
        .submit(
            runtime.agent_id().clone(),
            Input::Submitted {
                text: "continue".into(),
            },
        )
        .await
        .expect("settled preparation failure");
    assert!(!runtime.has_active_work());
    assert!(driver.calls().await.is_empty());
    assert_eq!(runtime.agent.journal().request_attempts().count(), attempts);
    assert!(runtime.agent.journal().records().iter().any(|record| matches!(record,
        JournalRecord::TurnFinished { fact, .. } if fact.outcome == plexmaton_agent::TurnOutcome::Failed)));
    runtime
        .restore_collaboration_context(collaboration.ledger())
        .expect("explicit source restoration remains possible");
    runtime.shutdown().await.expect("shutdown");
}

struct UnsupportedDriver(Arc<FakeDriver>);
impl crate::runtime::ModelDriver for UnsupportedDriver {
    fn request_environment(&self) -> &plexmaton_agent::RequestEnvironment {
        &self.0.environment
    }
    fn drive(
        &self,
        _attempt: plexmaton_agent::RequestAttemptId,
        _call: plexmaton_agent::ModelCall,
        _signals: tokio::sync::mpsc::Sender<crate::runtime::ModelSignal>,
        _cancellation: tokio_util::sync::CancellationToken,
    ) -> futures_util::future::BoxFuture<'static, crate::runtime::ModelTerminalReport> {
        panic!("unsupported driver must be refused before dispatch")
    }
}

/// CIN-3/CIN-4: driver support is explicit; refusal cannot write a session turn or request attempt.
#[tokio::test]
async fn cin_3_unsupported_driver_refuses_before_session_mutation() {
    let directory = Directory::new();
    let driver = Arc::new(UnsupportedDriver(FakeDriver::new([])));
    let (mut runtime, _, _, path) =
        runner(&directory.0, "b", driver, BoundaryGate::Inclusion).await;
    let mut collaboration = collaboration(&directory.0, dummy_main(), endpoint(&runtime));
    let control = collaboration
        .delegated_control(&delegation())
        .expect("child control");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control.clone())
        .expect("attach control");
    let reservation = control.reserve_execution().expect("reserve Main input");
    let source = admit(&mut collaboration, &runtime, "unsupported");
    let ticket = collaboration
        .execution_ticket(&delegation(), &source)
        .expect("ticket");
    let before = std::fs::read(&path).expect("session before refusal");
    let error = runtime
        .start_delegated_turn(reservation, ticket, Arc::clone(&source))
        .await
        .expect_err("unsupported context");
    assert!(matches!(
        error,
        RuntimeError::Collaboration(
            plexmaton_agent::collaboration::CollaborationError::UnsupportedContext
        )
    ));
    assert_eq!(std::fs::read(&path).expect("session after refusal"), before);
    assert!(!runtime.has_active_work());
    assert_eq!(runtime.agent.journal().request_attempts().count(), 0);
    let retry_ticket = collaboration
        .execution_ticket(&delegation(), &source)
        .expect("preflight refusal does not consume admission");
    let retry_reservation = control
        .reserve_execution()
        .expect("preflight refusal releases reservation");
    drop(
        retry_reservation
            .bind(retry_ticket)
            .expect("admission remains issuable"),
    );
    runtime.shutdown().await.expect("shutdown idle writer");
}
