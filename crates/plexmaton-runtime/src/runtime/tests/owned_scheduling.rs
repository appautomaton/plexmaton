use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use plexmaton_agent::collaboration::{
    CollaborationContext, CollaborationError, CollaborationEvent, CollaborationLimits,
    CollaborationText, DelegationController, DelegationRevision, MailEndpoint, MailEnvelope,
    TurnBoundary,
};
use plexmaton_agent::{
    Agent, ApprovalPolicy, ContextAtomValue, ConversationMetadata, HeadRevision, ModelEvent,
    StopReason, TurnBudget, UnixMillis,
};
use plexmaton_core::MailId;
use plexmaton_core::{
    AgentId, CollaborationId, CollaborationItemId, ConversationId, DelegationId, HeadName, TurnId,
};
use plexmaton_session_store::collaboration::CollaborationStoreError;
use plexmaton_session_store::collaboration::{CollaborationAttempt, CollaborationFile};
use plexmaton_session_store::{DelegatedConversationDirectory, JournalRecovery, StoreError};
use tokio::sync::Notify;

use super::tools::TestWorkspace;
use super::{FakeDriver, Script, text_delta};
use crate::runtime::{FixedWallClock, LiveRuntime};
use crate::{
    ChildStartError, CollaborationWriter, CollaborationWriterError, OwnedChildRunner,
    OwnedCollaboration, OwnedRunnerUpdate, OwnedSchedulingError, OwnedShutdownSettlement,
    PreparedChildExecution, RunnerGeneration, RunnerRegistrationReason, RuntimeUpdate,
    ScheduledTurnRequest, SchedulerLimits, WakeAdmission, WakeHint, WakeRefusal,
};

struct Directory(std::path::PathBuf);

impl Directory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("plexmaton-owned-runner-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir(&path).expect("create test directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("protect test directory");
        }
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove test directory");
    }
}

fn endpoint(name: &str) -> MailEndpoint {
    MailEndpoint {
        agent: AgentId::new(name).expect("agent"),
        conversation: ConversationId::new(format!("conversation-{name}")).expect("conversation"),
    }
}

fn collaboration_id() -> CollaborationId {
    CollaborationId::new("owned-runner").expect("collaboration")
}

fn delegation() -> DelegationId {
    DelegationId::new("task").expect("delegation")
}

fn item(name: &str) -> CollaborationItemId {
    CollaborationItemId::new(name).expect("item")
}

fn collaboration(directory: &Directory) -> CollaborationFile {
    let mut file = CollaborationFile::create(
        directory.0.join("collaboration.jsonl"),
        collaboration_id(),
        CollaborationLimits::default(),
    )
    .expect("collaboration file");
    file.admit(
        item("create"),
        CollaborationEvent::DelegationCreated {
            delegation: delegation(),
            delegator: endpoint("main"),
            worker: endpoint("child"),
            task: CollaborationText::new("Inspect the parser").expect("task"),
        },
    )
    .expect("create delegation");
    file
}

fn schedule_request(name: &str) -> ScheduledTurnRequest {
    ScheduledTurnRequest::new(
        delegation(),
        item(name),
        TurnBoundary {
            recipient: endpoint("child"),
            head: HeadName::new("main").expect("head"),
            head_revision: HeadRevision::new(0),
            parent: None,
            turn: TurnId::new(name).expect("turn"),
        },
        None,
    )
}

async fn separate_prepared_execution() -> (Directory, CollaborationWriter, PreparedChildExecution) {
    let directory = Directory::new();
    let mut writer = CollaborationWriter::spawn(collaboration(&directory)).expect("extra writer");
    let prepared = writer
        .schedule(schedule_request("extra-turn"))
        .await
        .expect("extra prepared execution");
    (directory, writer, prepared)
}

fn named_delegation(name: &str) -> DelegationId {
    DelegationId::new(format!("task-{name}")).expect("delegation")
}

fn two_child_collaboration(directory: &Directory) -> CollaborationFile {
    let mut file = CollaborationFile::create(
        directory.0.join("owned-collaboration.jsonl"),
        collaboration_id(),
        CollaborationLimits::default(),
    )
    .expect("collaboration file");
    for name in ["one", "two"] {
        file.admit(
            item(&format!("create-{name}")),
            CollaborationEvent::DelegationCreated {
                delegation: named_delegation(name),
                delegator: endpoint("main"),
                worker: endpoint(name),
                task: CollaborationText::new(format!("Inspect {name}")).expect("delegation task"),
            },
        )
        .expect("create delegation");
    }
    file
}

async fn bound_runtime(
    directory: &Directory,
    workspace: &TestWorkspace,
    writer: &CollaborationWriter,
    name: &str,
    driver: Arc<FakeDriver>,
) -> (LiveRuntime, ScheduledTurnRequest) {
    let child = endpoint(name);
    let delegation = named_delegation(name);
    let control = writer
        .delegated_control(delegation.clone())
        .await
        .expect("delegated control");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let journal = children
        .create(child.conversation.clone(), UnixMillis::EPOCH)
        .expect("child journal");
    let metadata = ConversationMetadata::new(child.conversation.clone(), UnixMillis::EPOCH);
    let mut runtime = LiveRuntime::with_delegated_driver_store_and_clock(
        child.agent.clone(),
        format!("Child {name}"),
        driver,
        workspace.catalog(),
        metadata,
        Box::new(journal),
        Arc::new(FixedWallClock(UnixMillis::EPOCH)),
    )
    .await
    .expect("child runtime");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation, control)
        .expect("bind child");
    let turn = TurnId::new(format!("turn-{name}")).expect("turn");
    let (boundary, previous) = runtime
        .collaboration_boundary(turn)
        .expect("child boundary");
    let request = ScheduledTurnRequest::new(
        delegation,
        item(&format!("turn-{name}")),
        boundary,
        previous,
    );
    (runtime, request)
}

/// SCH-2/SCH-4: update backpressure cannot consume Stop capacity or retain its permit.
#[tokio::test]
async fn sch_2_stop_completes_while_the_update_lane_is_saturated() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-runner");
    let mut collaboration =
        CollaborationWriter::spawn(collaboration(&directory)).expect("collaboration writer");
    let control = collaboration
        .delegated_control(delegation())
        .await
        .expect("delegated control");
    let child = endpoint("child");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let journal = children
        .create(child.conversation.clone(), UnixMillis::EPOCH)
        .expect("child journal");
    let metadata = ConversationMetadata::new(child.conversation.clone(), UnixMillis::EPOCH);
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver = FakeDriver::new([Script::WaitForCancellation {
        started: Arc::clone(&started),
        finished: Arc::clone(&cancelled),
    }]);
    let mut runtime = LiveRuntime::with_delegated_driver_store_and_clock(
        child.agent.clone(),
        "Child".into(),
        driver,
        workspace.catalog(),
        metadata,
        Box::new(journal),
        Arc::new(FixedWallClock(UnixMillis::EPOCH)),
    )
    .await
    .expect("child runtime");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control)
        .expect("bind child");
    let turn = TurnId::new("turn").expect("turn");
    let (boundary, previous) = runtime
        .collaboration_boundary(turn)
        .expect("idle child boundary");
    let prepared = collaboration
        .schedule(ScheduledTurnRequest::new(
            delegation(),
            item("turn"),
            boundary,
            previous,
        ))
        .await
        .expect("prepare child turn");
    let generation = RunnerGeneration::new(1).expect("generation");
    let mut runner = OwnedChildRunner::spawn(runtime, generation).expect("owned child");

    let announcement = runner.next_update().await.expect("initial tagged update");
    assert_eq!(announcement.identity().endpoint(), &child);
    assert_eq!(announcement.identity().generation(), generation);

    runner.start(prepared).await.expect("start child turn");
    let (_queued_directory, mut queued_writer, queued_execution) =
        separate_prepared_execution().await;
    let (_busy_directory, mut busy_writer, busy_execution) = separate_prepared_execution().await;
    let queued_result = {
        runner
            .begin_start(queued_execution)
            .expect("first queued start enters the bounded lane");
        tokio::task::yield_now().await;
        let busy = runner
            .begin_start(busy_execution)
            .expect_err("second queued start is refused immediately");
        let rejected = busy
            .into_unaccepted()
            .expect("busy start returns exact authority");
        drop(rejected);
        tokio::time::timeout(Duration::from_secs(5), runner.stop())
            .await
            .expect("Stop is never blocked by update backpressure")
            .expect("stop child");
        runner.finish_start().await
    };
    assert!(matches!(
        queued_result,
        Err(ChildStartError::Runtime(
            crate::RuntimeError::DelegatedControlBusy
        ))
    ));
    assert!(cancelled.load(Ordering::SeqCst));

    for writer in [&mut queued_writer, &mut busy_writer] {
        writer
            .admit_handoff(CollaborationAttempt {
                id: item("handoff"),
                event: CollaborationEvent::HandoffCompleted {
                    delegation: delegation(),
                    expected: DelegationRevision(0),
                    author: endpoint("main"),
                },
            })
            .await
            .expect("queued authority was disposed");
        writer.shutdown().await.expect("join extra writer");
    }

    collaboration
        .admit_handoff(CollaborationAttempt {
            id: item("handoff"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: delegation(),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("stop released execution authority");
    let update = runner.next_update().await.expect("tagged update");
    assert_eq!(update.identity().endpoint(), &child);
    assert_eq!(update.identity().generation(), generation);
    assert!(matches!(update, OwnedRunnerUpdate::Runtime { .. }));

    runner.begin_shutdown().expect("begin child shutdown");
    assert!(
        tokio::time::timeout(Duration::ZERO, runner.finish_shutdown())
            .await
            .is_err(),
        "cancelled shutdown wait remains resumable while output is backpressured"
    );
    loop {
        let update = runner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    runner.finish_shutdown().await.expect("join child");
    collaboration.shutdown().await.expect("join collaboration");
}

/// SCH-4: dropping an idle handle aborts its task and releases the child journal owner.
#[tokio::test]
async fn sch_4_handle_drop_cooperatively_releases_the_child_writer() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-runner-drop");
    let mut collaboration =
        CollaborationWriter::spawn(collaboration(&directory)).expect("collaboration writer");
    let control = collaboration
        .delegated_control(delegation())
        .await
        .expect("delegated control");
    let child = endpoint("child");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let journal = children
        .create(child.conversation.clone(), UnixMillis::EPOCH)
        .expect("child journal");
    let metadata = ConversationMetadata::new(child.conversation.clone(), UnixMillis::EPOCH);
    let mut runtime = LiveRuntime::with_delegated_driver_store_and_clock(
        child.agent.clone(),
        "Child".into(),
        FakeDriver::new([]),
        workspace.catalog(),
        metadata,
        Box::new(journal),
        Arc::new(FixedWallClock(UnixMillis::EPOCH)),
    )
    .await
    .expect("child runtime");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control)
        .expect("bind child");
    let runner = OwnedChildRunner::spawn(runtime, RunnerGeneration::new(1).expect("generation"))
        .expect("owned child");
    drop(runner);

    let reopened = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match children.resume(&child.conversation) {
                Ok(journal) => break journal,
                Err(StoreError::WriterLocked) => tokio::task::yield_now().await,
                Err(error) => panic!("reopen child after drop: {error}"),
            }
        }
    })
    .await
    .expect("cooperative drop released child writer");
    drop(reopened);
    collaboration.shutdown().await.expect("join collaboration");
}

/// SCH-4: dropping an active handle aborts its task and releases runtime-owned authority.
#[tokio::test]
async fn sch_4_active_handle_drop_aborts_without_detaching_runtime_authority() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-runner-active-drop");
    let mut collaboration =
        CollaborationWriter::spawn(collaboration(&directory)).expect("collaboration writer");
    let control = collaboration
        .delegated_control(delegation())
        .await
        .expect("delegated control");
    let child = endpoint("child");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let journal = children
        .create(child.conversation.clone(), UnixMillis::EPOCH)
        .expect("child journal");
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut runtime = LiveRuntime::with_delegated_driver_store_and_clock(
        child.agent.clone(),
        "Child".into(),
        FakeDriver::new([Script::WaitForCancellation {
            started: Arc::clone(&started),
            finished: Arc::clone(&cancelled),
        }]),
        workspace.catalog(),
        ConversationMetadata::new(child.conversation.clone(), UnixMillis::EPOCH),
        Box::new(journal),
        Arc::new(FixedWallClock(UnixMillis::EPOCH)),
    )
    .await
    .expect("child runtime");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control)
        .expect("bind child");
    let (boundary, previous) = runtime
        .collaboration_boundary(TurnId::new("drop-turn").expect("turn"))
        .expect("child boundary");
    let prepared = collaboration
        .schedule(ScheduledTurnRequest::new(
            delegation(),
            item("drop-turn"),
            boundary,
            previous,
        ))
        .await
        .expect("prepare child turn");
    let mut runner =
        OwnedChildRunner::spawn(runtime, RunnerGeneration::new(1).expect("generation"))
            .expect("owned child");
    runner.next_update().await.expect("initial update");
    runner.start(prepared).await.expect("start child");
    started.notified().await;
    drop(runner);

    let reopened = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match children.resume(&child.conversation) {
                Ok(journal) => break journal,
                Err(StoreError::WriterLocked) => tokio::task::yield_now().await,
                Err(error) => panic!("reopen active child after drop: {error}"),
            }
        }
    })
    .await
    .expect("active owner joined and released child writer");
    drop(reopened);
    collaboration
        .admit_handoff(CollaborationAttempt {
            id: item("handoff-after-drop"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: delegation(),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("active drop released execution authority");
    collaboration.shutdown().await.expect("join collaboration");
}

/// SCH-1/SCH-4: actor panic joins active runtime cleanup before authority becomes reusable.
#[tokio::test]
async fn sch_4_actor_panic_joins_active_provider_and_releases_authority() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-runner-panic");
    let mut collaboration =
        CollaborationWriter::spawn(collaboration(&directory)).expect("collaboration writer");
    let control = collaboration
        .delegated_control(delegation())
        .await
        .expect("delegated control");
    let child = endpoint("child");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let journal = children
        .create(child.conversation.clone(), UnixMillis::EPOCH)
        .expect("child journal");
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut runtime = LiveRuntime::with_delegated_driver_store_and_clock(
        child.agent.clone(),
        "Child".into(),
        FakeDriver::new([Script::WaitForCancellation {
            started: Arc::clone(&started),
            finished: Arc::clone(&cancelled),
        }]),
        workspace.catalog(),
        ConversationMetadata::new(child.conversation.clone(), UnixMillis::EPOCH),
        Box::new(journal),
        Arc::new(FixedWallClock(UnixMillis::EPOCH)),
    )
    .await
    .expect("child runtime");
    runtime
        .attach_delegated_control(&collaboration_id(), &delegation(), control)
        .expect("bind child");
    let (boundary, previous) = runtime
        .collaboration_boundary(TurnId::new("panic-turn").expect("turn"))
        .expect("child boundary");
    let prepared = collaboration
        .schedule(ScheduledTurnRequest::new(
            delegation(),
            item("panic-turn"),
            boundary,
            previous,
        ))
        .await
        .expect("prepare child turn");
    let mut runner =
        OwnedChildRunner::spawn(runtime, RunnerGeneration::new(1).expect("generation"))
            .expect("owned child");
    runner.next_update().await.expect("initial update");
    runner.start(prepared).await.expect("start child");
    started.notified().await;
    runner.panic_for_test();

    while runner.next_update().await.is_some() {}
    assert!(matches!(
        runner.join().await,
        Err(crate::OwnedRunnerError::WorkerFailed)
    ));
    assert!(cancelled.load(Ordering::SeqCst));
    collaboration
        .admit_handoff(CollaborationAttempt {
            id: item("handoff-after-panic"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: delegation(),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("panic cleanup released execution authority");
    collaboration.shutdown().await.expect("join collaboration");
}

/// SCH-2/SCH-4: the composition owner reports an unexpected child task failure explicitly.
#[tokio::test]
async fn sch_4_owner_surfaces_runner_panic_instead_of_clean_stream_closure() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-owner-panic");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let (runtime, _) =
        bound_runtime(&directory, &workspace, &writer, "one", FakeDriver::new([])).await;
    let conversation = runtime.conversation_id().clone();
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let identity = owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");

    owner.panic_runner_for_test(&conversation);
    let update = tokio::time::timeout(Duration::from_secs(5), owner.next_update())
        .await
        .expect("owner observes runner panic")
        .expect("typed failure update");
    assert!(update.is_finished());
    assert!(matches!(
        update,
        OwnedRunnerUpdate::WorkerFailed { identity: ref seen } if seen == &identity
    ));

    owner.begin_shutdown().await.expect("begin shutdown");
    let failure = owner
        .finish_shutdown()
        .await
        .expect_err("runner failure remains the shutdown result");
    assert!(matches!(
        failure.source(),
        OwnedSchedulingError::Control(crate::OwnedRunnerError::WorkerFailed)
    ));
    assert!(failure.report().settlements().is_empty());
    let repeated = owner
        .finish_shutdown()
        .await
        .expect_err("terminal shutdown never changes into success");
    assert!(matches!(
        repeated.source(),
        OwnedSchedulingError::ShutdownFinished
    ));
}

/// SCH-2/SCH-4: cancelling terminal observation retains the exact join for the next poll.
#[tokio::test]
async fn sch_2_cancelled_terminal_join_resumes_before_terminal_publication() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-cancelled-terminal-join");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let (runtime, _) =
        bound_runtime(&directory, &workspace, &writer, "one", FakeDriver::new([])).await;
    let conversation = runtime.conversation_id().clone();
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    let (join_entered, join_release) = owner.hold_runner_join_for_test(&conversation);

    owner.begin_shutdown().await.expect("begin shutdown");
    {
        let terminal = owner.next_update();
        tokio::pin!(terminal);
        tokio::select! {
            () = join_entered.notified() => {}
            update = &mut terminal => panic!("terminal escaped before join completed: {update:?}"),
        }
    }

    join_release.notify_one();
    let terminal = tokio::time::timeout(Duration::from_secs(5), owner.next_update())
        .await
        .expect("retained join resumes")
        .expect("terminal update remains retained");
    assert!(terminal.is_finished());
    let children = DelegatedConversationDirectory::under(&directory.0)
        .expect("delegated directory after retained join");
    let reopened = children
        .resume(&conversation)
        .expect("terminal publication follows child writer release");
    drop(reopened);

    let report = owner.finish_shutdown().await.expect("joined shutdown");
    assert_eq!(report.len(), 1);
}

/// SCH-2/SCH-3/SCH-4: one owner bounds runners, rejects stale routing and joins Handoff/shutdown.
#[tokio::test]
async fn sch_4_owner_schedules_and_hands_off_only_after_child_quiescence() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-collaboration");
    let collaboration_path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let (first, first_request) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "one",
        FakeDriver::new([Script::WaitForCancellation {
            started: Arc::clone(&started),
            finished: Arc::clone(&cancelled),
        }]),
    )
    .await;
    let (second, _second_request) =
        bound_runtime(&directory, &workspace, &writer, "two", FakeDriver::new([])).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let first_identity = owner.register(first).await.expect("register first child");
    let refused = owner
        .register(second)
        .await
        .expect_err("runner capacity is independent and bounded");
    assert!(matches!(
        refused.reason(),
        RunnerRegistrationReason::Capacity
    ));
    let mut second = refused.into_runtime();
    second
        .shutdown()
        .await
        .expect("shutdown unregistered child");

    let initial = owner.next_update().await.expect("initial child update");
    assert!(owner.accepts_update(&initial));
    assert_eq!(initial.identity(), &first_identity);
    let stale = OwnedRunnerUpdate::Runtime {
        identity: initial
            .identity()
            .with_generation_for_test(RunnerGeneration::new(2).expect("stale generation")),
        update: Box::new(RuntimeUpdate::Finished),
    };
    assert!(!owner.accepts_update(&stale));
    owner
        .schedule(first_request.clone())
        .await
        .expect("schedule child");
    owner
        .handoff(CollaborationAttempt {
            id: item("handoff-one"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: named_delegation("one"),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("Handoff stops and joins child work");
    assert!(cancelled.load(Ordering::SeqCst));

    let mut boundary = first_request.boundary().clone();
    boundary.turn = TurnId::new("turn-one-after-handoff").expect("turn");
    let after_handoff = ScheduledTurnRequest::new(
        named_delegation("one"),
        item("turn-one-after-handoff"),
        boundary,
        None,
    );
    let refusal = owner
        .schedule(after_handoff)
        .await
        .expect_err("Handoff permanently closes Main scheduling");
    assert!(matches!(
        refusal.source(),
        OwnedSchedulingError::HandoffPending
    ));
    let wake_refusal = owner
        .wake(WakeHint::new(
            first_identity,
            item("wake-after-handoff"),
            TurnId::new("wake-after-handoff").expect("turn"),
        ))
        .expect_err("Handoff permanently closes wake admission");
    assert_eq!(wake_refusal.reason(), &WakeRefusal::HandoffPending);
    let first_conversation = first_request.boundary().recipient.conversation.clone();

    owner.begin_shutdown().await.expect("begin owner shutdown");
    loop {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    let children = DelegatedConversationDirectory::under(&directory.0)
        .expect("delegated directory after Finished");
    let reopened = children
        .resume(&first_conversation)
        .expect("Finished is observed only after the child owner has joined");
    drop(reopened);
    let reports = owner.finish_shutdown().await.expect("join owner");
    assert_eq!(reports.len(), 1);

    let reopened = CollaborationFile::open(collaboration_path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .delegation(&named_delegation("one"))
            .expect("first delegation")
            .controller,
        DelegationController::User
    );
    assert_eq!(
        reopened
            .ledger()
            .delegation(&named_delegation("two"))
            .expect("second delegation")
            .controller,
        DelegationController::Main
    );
}

/// SCH-2/SCH-4: two bounded runner tasks make independent progress and join through one owner.
#[tokio::test]
async fn sch_2_owner_multiplexes_two_independent_runners() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-two-runners");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let first_driver = FakeDriver::new([Script::Events(vec![
        text_delta("first child"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let second_driver = FakeDriver::new([Script::Events(vec![
        text_delta("second child"),
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
    let (second, second_request) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "two",
        Arc::clone(&second_driver),
    )
    .await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(2).expect("limits"));
    let first_identity = owner.register(first).await.expect("register first");
    let second_identity = owner.register(second).await.expect("register second");

    let mut announced = std::collections::BTreeSet::new();
    while announced.len() < 2 {
        let update = owner.next_update().await.expect("announcement");
        announced.insert(update.identity().endpoint().conversation.clone());
    }
    assert!(announced.contains(&first_identity.endpoint().conversation));
    assert!(announced.contains(&second_identity.endpoint().conversation));

    owner.schedule(first_request).await.expect("schedule first");
    owner
        .schedule(second_request)
        .await
        .expect("schedule second");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !first_driver.calls().await.is_empty() && !second_driver.calls().await.is_empty() {
                break;
            }
            owner.next_update().await.expect("runner progress");
        }
    })
    .await
    .expect("both providers progress independently");

    owner.begin_shutdown().await.expect("begin owner shutdown");
    let mut finished = std::collections::BTreeSet::new();
    while finished.len() < 2 {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            finished.insert(update.identity().endpoint().conversation.clone());
        }
    }
    let reports = owner.finish_shutdown().await.expect("join owner");
    assert_eq!(reports.len(), 2);
}

/// SCH-1/SCH-3: cancellation cannot strand durable admission before the reserved runner receives it.
#[tokio::test]
async fn sch_1_cancelled_owner_schedule_resumes_the_exact_accepted_command() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-schedule-cancellation");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let driver = FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
        StopReason::EndOfTurn,
    )])]);
    let (runtime, request) =
        bound_runtime(&directory, &workspace, &writer, "one", Arc::clone(&driver)).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    let (entered, release) = owner.hold_writer_for_test();
    entered.recv().expect("writer is blocked");

    assert!(
        tokio::time::timeout(Duration::ZERO, owner.schedule(request.clone()))
            .await
            .is_err(),
        "the caller wait is cancelled after the command and runner slot are retained"
    );
    release.send(()).expect("release writer");
    owner
        .schedule(request)
        .await
        .expect("exact retry resumes the retained schedule");

    tokio::time::timeout(Duration::from_secs(5), async {
        while driver.calls().await.is_empty() {
            owner.next_update().await.expect("child progress");
        }
    })
    .await
    .expect("retained schedule reaches the provider exactly once");
    assert_eq!(driver.calls().await.len(), 1);

    owner.begin_shutdown().await.expect("begin shutdown");
    while let Some(update) = owner.next_update().await {
        if update.is_finished() {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join owner");
}

/// SCH-1: a cancelled regular admission exposes its exact acknowledged result without caller input.
#[tokio::test]
async fn sch_1_cancelled_regular_admission_retains_its_completion() {
    let directory = Directory::new();
    let path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let (entered, release) = owner.hold_writer_for_test();
    entered.recv().expect("writer is blocked");
    let attempt = CollaborationAttempt {
        id: item("cancelled-update"),
        event: CollaborationEvent::TaskUpdated {
            delegation: named_delegation("one"),
            expected: DelegationRevision(0),
            author: endpoint("main"),
            task: CollaborationText::new("Inspect the retained result").expect("task"),
        },
    };

    assert!(
        tokio::time::timeout(Duration::ZERO, owner.admit(attempt))
            .await
            .is_err(),
        "caller cancellation leaves the accepted admission with its owner"
    );
    release.send(()).expect("release writer");
    let receipt = owner
        .finish_admission()
        .await
        .expect("owner retained an accepted admission")
        .expect("admission completed");
    assert_eq!(receipt.id, item("cancelled-update"));
    owner.begin_shutdown().await.expect("begin shutdown");
    owner.finish_shutdown().await.expect("join owner");

    let reopened = CollaborationFile::open(path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .delegation(&named_delegation("one"))
            .expect("delegation")
            .task
            .as_str(),
        "Inspect the retained result"
    );
}

/// SCH-1/SCH-4: shutdown returns a cancelled regular admission result without caller replay.
#[tokio::test]
async fn sch_4_shutdown_returns_a_cancelled_regular_admission_result() {
    let directory = Directory::new();
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let (entered, release) = owner.hold_writer_for_test();
    entered.recv().expect("writer is blocked");
    let attempt = CollaborationAttempt {
        id: item("shutdown-update"),
        event: CollaborationEvent::TaskUpdated {
            delegation: named_delegation("one"),
            expected: DelegationRevision(0),
            author: endpoint("main"),
            task: CollaborationText::new("Settle this during shutdown").expect("task"),
        },
    };
    assert!(
        tokio::time::timeout(Duration::ZERO, owner.admit(attempt))
            .await
            .is_err(),
        "caller cancellation leaves admission owned"
    );
    release.send(()).expect("release writer");

    owner.begin_shutdown().await.expect("begin shutdown");
    let shutdown = owner.finish_shutdown().await.expect("join owner");
    assert!(matches!(
        shutdown.settlements(),
        [OwnedShutdownSettlement::Admission(Ok(receipt))]
            if receipt.id == item("shutdown-update")
    ));
}

/// SCH-1/SCH-4: Handoff retains its exact attempt before asynchronous preflight acknowledgement.
#[tokio::test]
async fn sch_1_cancelled_handoff_preflight_resumes_the_owned_attempt() {
    let directory = Directory::new();
    let path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let attempt = CollaborationAttempt {
        id: item("cancelled-preflight-handoff"),
        event: CollaborationEvent::HandoffCompleted {
            delegation: named_delegation("one"),
            expected: DelegationRevision(0),
            author: endpoint("main"),
        },
    };
    let (entered, release) = owner.hold_writer_for_test();
    entered.recv().expect("writer is blocked");

    assert!(
        tokio::time::timeout(Duration::ZERO, owner.handoff(attempt.clone()))
            .await
            .is_err(),
        "caller cancellation leaves Handoff owned before preflight reply"
    );
    release.send(()).expect("release writer");
    loop {
        match owner.handoff(attempt.clone()).await {
            Ok(_) => break,
            Err(failure)
                if matches!(
                    failure.source(),
                    OwnedSchedulingError::Writer(
                        CollaborationWriterError::Busy
                            | CollaborationWriterError::AdmissionBusy { .. }
                            | CollaborationWriterError::AdmissionInProgress { .. }
                    )
                ) =>
            {
                tokio::task::yield_now().await;
            }
            Err(failure) => panic!("resume Handoff preflight: {failure}"),
        }
    }
    owner.begin_shutdown().await.expect("begin shutdown");
    owner.finish_shutdown().await.expect("join owner");
    let reopened = CollaborationFile::open(path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .delegation(&named_delegation("one"))
            .expect("delegation")
            .controller,
        DelegationController::User
    );
}

/// SCH-4: shutdown settles an internally retained schedule without requiring a caller copy.
#[tokio::test]
async fn sch_4_shutdown_settles_a_cancelled_schedule_without_its_request_token() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-shutdown-pending-schedule");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let (runtime, request) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "one",
        FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
            StopReason::EndOfTurn,
        )])]),
    )
    .await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    let (entered, release) = owner.hold_writer_for_test();
    entered.recv().expect("writer is blocked");

    assert!(
        tokio::time::timeout(Duration::ZERO, owner.schedule(request))
            .await
            .is_err(),
        "caller drops its only schedule request after owner admission"
    );
    release.send(()).expect("release writer");
    owner
        .begin_shutdown()
        .await
        .expect("shutdown owns pending schedule settlement");
    loop {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    let shutdown = owner
        .finish_shutdown()
        .await
        .expect("join owner and writer");
    assert!(matches!(
        shutdown.settlements(),
        [OwnedShutdownSettlement::Schedule(Ok(_))]
    ));
}

/// SCH-4: shutdown resumes a canceled Stop without another call or retained target token.
#[tokio::test]
async fn sch_4_shutdown_settles_a_cancelled_stop_without_caller_replay() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-shutdown-pending-stop");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (runtime, request) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "one",
        FakeDriver::new([Script::WaitForCancellationAndRelease {
            started: Arc::clone(&started),
            cancelled: Arc::clone(&cancelled),
            release: Arc::clone(&release),
        }]),
    )
    .await;
    let conversation = runtime.conversation_id().clone();
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    owner.schedule(request).await.expect("schedule child");
    started.notified().await;

    {
        let stop = owner.stop(&conversation);
        tokio::pin!(stop);
        tokio::select! {
            () = cancelled.notified() => {}
            result = &mut stop => panic!("Stop settled before the cancellation gate: {result:?}"),
        }
    }
    release.notify_one();
    owner
        .begin_shutdown()
        .await
        .expect("shutdown resumes the retained Stop");
    loop {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    let shutdown = owner
        .finish_shutdown()
        .await
        .expect("join owner and writer");
    assert!(matches!(
        shutdown.settlements(),
        [OwnedShutdownSettlement::Stop(Ok(_))]
    ));
}

/// SCH-4: shutdown resumes a canceled Handoff without requiring its moved attempt again.
#[tokio::test]
async fn sch_4_shutdown_settles_a_cancelled_handoff_without_caller_replay() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-shutdown-pending-handoff");
    let collaboration_path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (runtime, request) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "one",
        FakeDriver::new([Script::WaitForCancellationAndRelease {
            started: Arc::clone(&started),
            cancelled: Arc::clone(&cancelled),
            release: Arc::clone(&release),
        }]),
    )
    .await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    owner.schedule(request).await.expect("schedule child");
    started.notified().await;

    {
        let handoff = owner.handoff(CollaborationAttempt {
            id: item("cancelled-handoff"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: named_delegation("one"),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        });
        tokio::pin!(handoff);
        tokio::select! {
            () = cancelled.notified() => {}
            result = &mut handoff => panic!("Handoff settled before the cancellation gate: {result:?}"),
        }
    }
    release.notify_one();
    owner
        .begin_shutdown()
        .await
        .expect("shutdown resumes the retained Handoff");
    loop {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    let shutdown = owner
        .finish_shutdown()
        .await
        .expect("join owner and writer");
    assert!(matches!(
        shutdown.settlements(),
        [OwnedShutdownSettlement::Handoff(Ok(_))]
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

/// SCH-4: an invalid Handoff is refused before it closes admission or interrupts child work.
#[tokio::test]
async fn sch_4_handoff_preflight_precedes_stop_and_retains_its_report() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-handoff-preflight");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let (runtime, request) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "one",
        FakeDriver::new([Script::WaitForCancellation {
            started: Arc::clone(&started),
            finished: Arc::clone(&cancelled),
        }]),
    )
    .await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    owner.schedule(request).await.expect("schedule child");
    started.notified().await;

    let invalid = CollaborationAttempt {
        id: item("invalid-handoff"),
        event: CollaborationEvent::HandoffCompleted {
            delegation: named_delegation("one"),
            expected: DelegationRevision(0),
            author: endpoint("intruder"),
        },
    };
    let failure = owner
        .handoff(invalid)
        .await
        .expect_err("foreign Handoff fails canonical preflight");
    assert!(matches!(failure.source(), OwnedSchedulingError::Writer(_)));
    assert!(!cancelled.load(Ordering::SeqCst));

    let handoff = owner
        .handoff(CollaborationAttempt {
            id: item("valid-handoff"),
            event: CollaborationEvent::HandoffCompleted {
                delegation: named_delegation("one"),
                expected: DelegationRevision(0),
                author: endpoint("main"),
            },
        })
        .await
        .expect("valid Handoff stops then appends");
    assert!(handoff.stopped.is_some());
    assert!(cancelled.load(Ordering::SeqCst));

    owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join owner");
}

/// SCH-3/PRV-1: unsupported provider capability refuses before durable turn admission.
#[tokio::test]
async fn sch_3_provider_refusal_precedes_collaboration_admission() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-provider-refusal");
    let path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let driver = FakeDriver::without_collaboration([]);
    let (runtime, request) =
        bound_runtime(&directory, &workspace, &writer, "one", Arc::clone(&driver)).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let identity = owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    let before = std::fs::read(&path).expect("collaboration bytes");

    let wake_failure = owner
        .wake(WakeHint::new(
            identity,
            item("unsupported-wake"),
            TurnId::new("unsupported-wake").expect("turn"),
        ))
        .expect_err("unsupported provider refuses wake admission");
    assert_eq!(wake_failure.reason(), &WakeRefusal::ProviderUnsupported);
    assert_eq!(std::fs::read(&path).expect("unchanged wake bytes"), before);
    assert!(driver.calls().await.is_empty());

    let failure = owner
        .schedule(request)
        .await
        .expect_err("unsupported provider refuses scheduling");
    assert!(matches!(
        failure.source(),
        OwnedSchedulingError::ProviderUnsupported
    ));
    assert_eq!(std::fs::read(&path).expect("unchanged bytes"), before);
    assert!(driver.calls().await.is_empty());

    owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join owner");
}

/// SCH-5/CIN-4: a coalesced wake rereads late canonical facts and admits one exact turn.
#[tokio::test]
async fn sch_5_wake_coalesces_and_rereads_the_canonical_prefix() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-canonical-wake");
    let path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let driver = FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
        StopReason::EndOfTurn,
    )])]);
    let (runtime, _) =
        bound_runtime(&directory, &workspace, &writer, "one", Arc::clone(&driver)).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let identity = owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");

    assert_eq!(
        owner
            .wake(WakeHint::new(
                identity.clone(),
                item("wake-admission-one"),
                TurnId::new("wake-turn-one").expect("turn"),
            ))
            .expect("retain wake"),
        WakeAdmission::Accepted
    );
    assert_eq!(
        owner
            .wake(WakeHint::new(
                identity.clone(),
                item("coalesced-wake"),
                TurnId::new("coalesced-turn").expect("turn"),
            ))
            .expect("coalesce duplicate wake"),
        WakeAdmission::Coalesced
    );

    owner
        .admit(CollaborationAttempt {
            id: item("late-mail"),
            event: CollaborationEvent::MailAccepted {
                mail: MailEnvelope {
                    id: MailId::new("late-mail").expect("mail"),
                    from: endpoint("main"),
                    to: endpoint("one"),
                    summary: CollaborationText::new("Late canonical evidence").expect("mail text"),
                    artifacts: Vec::new(),
                },
            },
        })
        .await
        .expect("append fact after advisory hint");

    let update = tokio::time::timeout(Duration::from_secs(5), owner.next_update())
        .await
        .expect("wake progresses")
        .expect("wake update");
    assert!(matches!(
        update,
        OwnedRunnerUpdate::WakeScheduled { identity: ref seen, .. } if seen == &identity
    ));
    let calls = driver.calls().await;
    assert_eq!(calls.len(), 1);
    let resolved = calls[0]
        .request
        .atoms
        .iter()
        .find_map(|atom| match atom.value() {
            ContextAtomValue::Collaboration(CollaborationContext::Resolved(resolved)) => {
                Some(resolved)
            }
            _ => None,
        })
        .expect("wake dispatch contains resolved canonical context");
    assert!(
        resolved
            .items()
            .iter()
            .any(|source| source.reference.item == item("late-mail"))
    );

    owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join owner");

    let reopened = CollaborationFile::open(path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .records()
            .iter()
            .filter(|record| matches!(record.event, CollaborationEvent::TurnAdmitted { .. }))
            .count(),
        1
    );
}

/// SCH-2/SCH-5: an advisory wake cannot cross a retired runner generation.
#[tokio::test]
async fn sch_5_wake_rejects_a_stale_runner_generation_without_mutation() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-stale-wake");
    let path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let driver = FakeDriver::new([]);
    let (runtime, _) = bound_runtime(&directory, &workspace, &writer, "one", driver).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let identity = owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    let before = std::fs::read(&path).expect("collaboration bytes");
    let stale_generation = if identity.generation().get() == u64::MAX {
        RunnerGeneration::new(u64::MAX - 1).expect("generation")
    } else {
        RunnerGeneration::new(identity.generation().get() + 1).expect("generation")
    };
    let stale = identity.with_generation_for_test(stale_generation);

    let failure = owner
        .wake(WakeHint::new(
            stale,
            item("stale-wake"),
            TurnId::new("stale-wake").expect("turn"),
        ))
        .expect_err("stale wake is refused");
    assert_eq!(failure.reason(), &WakeRefusal::StaleRunner);
    assert_eq!(std::fs::read(&path).expect("unchanged bytes"), before);

    owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join owner");
}

/// SCH-1/SCH-5: cancellation retains the exact wake-derived scheduling command in the owner.
#[tokio::test]
async fn sch_5_cancelled_wake_resumes_without_a_second_admission() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-cancelled-wake");
    let path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let driver = FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
        StopReason::EndOfTurn,
    )])]);
    let (runtime, _) =
        bound_runtime(&directory, &workspace, &writer, "one", Arc::clone(&driver)).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let identity = owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    let (entered, release) = owner.hold_writer_for_test();
    entered.recv().expect("writer is blocked");
    owner
        .wake(WakeHint::new(
            identity,
            item("cancelled-wake-admission"),
            TurnId::new("cancelled-wake-turn").expect("turn"),
        ))
        .expect("retain wake");
    tokio::task::yield_now().await;

    assert!(
        tokio::time::timeout(Duration::from_millis(10), owner.next_update())
            .await
            .is_err(),
        "caller cancellation leaves the accepted wake schedule in its owner"
    );
    release.send(()).expect("release writer");
    let update = tokio::time::timeout(Duration::from_secs(5), owner.next_update())
        .await
        .expect("retained wake resumes")
        .expect("wake update");
    assert!(matches!(update, OwnedRunnerUpdate::WakeScheduled { .. }));
    assert_eq!(driver.calls().await.len(), 1);

    owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join owner");
    let reopened = CollaborationFile::open(path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .records()
            .iter()
            .filter(|record| matches!(record.event, CollaborationEvent::TurnAdmitted { .. }))
            .count(),
        1
    );
}

/// SCH-2/SCH-5: a busy child retains one coalesced wake until its current turn is quiescent.
#[tokio::test]
async fn sch_5_busy_child_runs_one_retained_wake_after_quiescence() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-busy-wake");
    let path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let driver = FakeDriver::new([
        Script::WaitForRelease {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        },
        Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
    ]);
    let (runtime, first_request) =
        bound_runtime(&directory, &workspace, &writer, "one", Arc::clone(&driver)).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let identity = owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    owner
        .schedule(first_request)
        .await
        .expect("start first turn");
    started.notified().await;

    assert_eq!(
        owner
            .wake(WakeHint::new(
                identity.clone(),
                item("wake-after-busy"),
                TurnId::new("wake-after-busy").expect("turn"),
            ))
            .expect("retain wake while active"),
        WakeAdmission::Accepted
    );
    assert_eq!(
        owner
            .wake(WakeHint::new(
                identity,
                item("coalesced-while-busy"),
                TurnId::new("coalesced-while-busy").expect("turn"),
            ))
            .expect("coalesce active wake"),
        WakeAdmission::Coalesced
    );
    owner
        .admit(CollaborationAttempt {
            id: item("mail-while-busy"),
            event: CollaborationEvent::MailAccepted {
                mail: MailEnvelope {
                    id: MailId::new("mail-while-busy").expect("mail"),
                    from: endpoint("main"),
                    to: endpoint("one"),
                    summary: CollaborationText::new("New work while the child is active")
                        .expect("mail text"),
                    artifacts: Vec::new(),
                },
            },
        })
        .await
        .expect("append work for the retained wake");
    release.notify_one();

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let update = owner.next_update().await.expect("runner progress");
            if matches!(update, OwnedRunnerUpdate::WakeScheduled { .. }) {
                break;
            }
        }
    })
    .await
    .expect("wake starts after first turn becomes quiescent");
    assert_eq!(driver.calls().await.len(), 2);

    owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        let update = owner.next_update().await.expect("shutdown update");
        if update.is_finished() {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join owner");
    let reopened = CollaborationFile::open(path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .records()
            .iter()
            .filter(|record| matches!(record.event, CollaborationEvent::TurnAdmitted { .. }))
            .count(),
        2
    );
}

/// SCH-2/SCH-5: a refused Stop for another child cannot cancel its retained wake.
#[tokio::test]
async fn sch_5_stop_in_progress_preserves_another_child_wake() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-stop-wake-isolation");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let first_driver = FakeDriver::new([Script::WaitForCancellationAndRelease {
        started: Arc::clone(&started),
        cancelled: Arc::clone(&cancelled),
        release: Arc::clone(&release),
    }]);
    let second_driver = FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
        StopReason::EndOfTurn,
    )])]);
    let (first, first_request) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "one",
        Arc::clone(&first_driver),
    )
    .await;
    let first_conversation = first.conversation_id().clone();
    let (second, _) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "two",
        Arc::clone(&second_driver),
    )
    .await;
    let second_conversation = second.conversation_id().clone();
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(2).expect("limits"));
    owner.register(first).await.expect("register first");
    let second_identity = owner.register(second).await.expect("register second");
    for _ in 0..2 {
        owner.next_update().await.expect("initial update");
    }
    owner
        .schedule(first_request)
        .await
        .expect("start first child");
    started.notified().await;
    {
        let stop = owner.stop(&first_conversation);
        tokio::pin!(stop);
        tokio::select! {
            () = cancelled.notified() => {}
            result = &mut stop => panic!("Stop settled before its gate: {result:?}"),
        }
    }
    owner
        .wake(WakeHint::new(
            second_identity,
            item("second-child-wake"),
            TurnId::new("second-child-wake").expect("turn"),
        ))
        .expect("retain second wake");

    assert!(matches!(
        owner.stop(&second_conversation).await,
        Err(OwnedSchedulingError::StopInProgress)
    ));
    release.notify_one();
    let settled = owner.next_update().await.expect("settled first Stop");
    assert!(matches!(settled, OwnedRunnerUpdate::StopSettled { .. }));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(
                owner.next_update().await.expect("runner progress"),
                OwnedRunnerUpdate::WakeScheduled { .. }
            ) {
                break;
            }
        }
    })
    .await
    .expect("second wake survived refused Stop");
    assert_eq!(second_driver.calls().await.len(), 1);

    owner.begin_shutdown().await.expect("begin shutdown");
    let mut finished = 0;
    while finished < 2 {
        if owner
            .next_update()
            .await
            .expect("shutdown update")
            .is_finished()
        {
            finished += 1;
        }
    }
    owner.finish_shutdown().await.expect("join owner");
}

/// SCH-2/SCH-5: writer backpressure parks one wake until bounded command capacity reopens.
#[tokio::test]
async fn sch_5_writer_busy_wake_does_not_spin_or_starve_updates() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-writer-busy-wake");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let driver = FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
        StopReason::EndOfTurn,
    )])]);
    let (runtime, _) =
        bound_runtime(&directory, &workspace, &writer, "one", Arc::clone(&driver)).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let identity = owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    let (first_entered, first_release) = owner.hold_writer_for_test();
    first_entered.recv().expect("writer entered first hold");
    let (second_entered, second_release) = owner.hold_writer_for_test();
    let hint = WakeHint::new(
        identity,
        item("writer-busy-wake"),
        TurnId::new("writer-busy-wake").expect("turn"),
    );
    owner.wake(hint).expect("retain wake");

    let deferred = tokio::time::timeout(Duration::from_secs(5), owner.next_update())
        .await
        .expect("wake reaches writer backpressure")
        .expect("deferred update");
    assert!(matches!(deferred, OwnedRunnerUpdate::WakeDeferred { .. }));
    first_release.send(()).expect("release first hold");
    second_entered.recv().expect("writer entered second hold");
    let scheduled = {
        let progress = owner.next_update();
        tokio::pin!(progress);
        assert!(
            tokio::time::timeout(Duration::ZERO, &mut progress)
                .await
                .is_err(),
            "a deferred wake waits behind the accepted writer command"
        );
        second_release.send(()).expect("release second hold");
        tokio::time::timeout(Duration::from_secs(5), &mut progress)
            .await
            .expect("parked wake progresses when writer capacity reopens")
            .expect("wake update")
    };
    assert!(matches!(scheduled, OwnedRunnerUpdate::WakeScheduled { .. }));
    assert_eq!(driver.calls().await.len(), 1);

    owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        if owner
            .next_update()
            .await
            .expect("shutdown update")
            .is_finished()
        {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join owner");
}

/// SCH-5/COL-1: a colliding admission identity is a typed no-mutation wake refusal.
#[tokio::test]
async fn sch_5_wake_admission_identity_collision_never_dispatches() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-wake-collision");
    let path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let driver = FakeDriver::new([]);
    let (runtime, _) =
        bound_runtime(&directory, &workspace, &writer, "one", Arc::clone(&driver)).await;
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let identity = owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    let before = std::fs::read(&path).expect("collaboration bytes");

    owner
        .wake(WakeHint::new(
            identity,
            item("create-one"),
            TurnId::new("collision-turn").expect("turn"),
        ))
        .expect("advisory wake is retained before canonical recheck");
    let update = owner.next_update().await.expect("wake refusal");
    assert!(matches!(
        update.wake_rejection(),
        Some(OwnedSchedulingError::Writer(
            CollaborationWriterError::Schedule {
                source: CollaborationStoreError::Rejected(CollaborationError::ItemIdentityConflict),
                ..
            }
        ))
    ));
    assert_eq!(std::fs::read(&path).expect("unchanged bytes"), before);
    assert!(driver.calls().await.is_empty());

    owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        if owner
            .next_update()
            .await
            .expect("shutdown update")
            .is_finished()
        {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join owner");
}

/// SCH-2: runner generations remain distinct across replacement owner instances.
#[tokio::test]
async fn sch_2_runner_generation_does_not_restart_with_a_new_owner() {
    let first_directory = Directory::new();
    let first_workspace = TestWorkspace::new("owned-generation-first");
    let first_writer = CollaborationWriter::spawn(two_child_collaboration(&first_directory))
        .expect("first collaboration writer");
    let (first_runtime, _) = bound_runtime(
        &first_directory,
        &first_workspace,
        &first_writer,
        "one",
        FakeDriver::new([]),
    )
    .await;
    let mut first_owner =
        OwnedCollaboration::new(first_writer, SchedulerLimits::new(1).expect("limits"));
    let old_identity = first_owner
        .register(first_runtime)
        .await
        .expect("register first child");
    first_owner.next_update().await.expect("initial update");
    first_owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        if first_owner
            .next_update()
            .await
            .expect("shutdown update")
            .is_finished()
        {
            break;
        }
    }
    first_owner
        .finish_shutdown()
        .await
        .expect("join first owner");

    let second_directory = Directory::new();
    let second_workspace = TestWorkspace::new("owned-generation-second");
    let second_writer = CollaborationWriter::spawn(two_child_collaboration(&second_directory))
        .expect("second collaboration writer");
    let (second_runtime, _) = bound_runtime(
        &second_directory,
        &second_workspace,
        &second_writer,
        "one",
        FakeDriver::new([]),
    )
    .await;
    let mut second_owner =
        OwnedCollaboration::new(second_writer, SchedulerLimits::new(1).expect("limits"));
    let current_identity = second_owner
        .register(second_runtime)
        .await
        .expect("register replacement child");
    second_owner.next_update().await.expect("initial update");
    assert_ne!(old_identity.generation(), current_identity.generation());
    let stale = OwnedRunnerUpdate::Runtime {
        identity: old_identity,
        update: Box::new(RuntimeUpdate::Finished),
    };
    assert!(!second_owner.accepts_update(&stale));

    second_owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        if second_owner
            .next_update()
            .await
            .expect("shutdown update")
            .is_finished()
        {
            break;
        }
    }
    second_owner
        .finish_shutdown()
        .await
        .expect("join second owner");
}

/// SCH-5/CIN-2: registration restores prior branch-local admissions before a resumed wake.
#[tokio::test]
async fn sch_5_resumed_child_restores_context_before_wake_dispatch() {
    let directory = Directory::new();
    let workspace = TestWorkspace::new("owned-resumed-wake");
    let collaboration_path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let first_driver = FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
        StopReason::EndOfTurn,
    )])]);
    let (runtime, request) = bound_runtime(
        &directory,
        &workspace,
        &writer,
        "one",
        Arc::clone(&first_driver),
    )
    .await;
    let child = endpoint("one");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    owner.register(runtime).await.expect("register child");
    owner.next_update().await.expect("initial update");
    owner.schedule(request).await.expect("schedule first turn");
    tokio::time::timeout(Duration::from_secs(5), async {
        while first_driver.calls().await.is_empty() {
            owner.next_update().await.expect("first turn progress");
        }
    })
    .await
    .expect("first dispatch");
    owner.begin_shutdown().await.expect("begin first shutdown");
    loop {
        if owner
            .next_update()
            .await
            .expect("first shutdown update")
            .is_finished()
        {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join first owner");

    let collaboration = CollaborationFile::open(&collaboration_path).expect("reopen collaboration");
    let control = collaboration
        .delegated_control(&named_delegation("one"))
        .expect("reopen control");
    let children =
        DelegatedConversationDirectory::under(&directory.0).expect("delegated directory");
    let journal = children
        .resume(&child.conversation)
        .expect("reopen child journal");
    let agent = Agent::from_journal(
        child.agent.clone(),
        journal.journal().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .expect("restore child agent");
    let resumed_driver = FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
        StopReason::EndOfTurn,
    )])]);
    let (mut runtime, _) = LiveRuntime::with_resumed_delegated_driver_and_store(
        child.agent.clone(),
        agent,
        resumed_driver.clone(),
        workspace.catalog(),
        Box::new(journal),
        JournalRecovery::Clean,
        Arc::new(FixedWallClock(UnixMillis::EPOCH)),
    )
    .await
    .expect("resume child runtime");
    runtime
        .attach_delegated_control(&collaboration_id(), &named_delegation("one"), control)
        .expect("reattach control");
    let writer = CollaborationWriter::spawn(collaboration).expect("resume collaboration writer");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    let identity = owner
        .register(runtime)
        .await
        .expect("registration restores collaboration context");
    owner
        .admit(CollaborationAttempt {
            id: item("mail-after-resume"),
            event: CollaborationEvent::MailAccepted {
                mail: MailEnvelope {
                    id: MailId::new("mail-after-resume").expect("mail"),
                    from: endpoint("main"),
                    to: child,
                    summary: CollaborationText::new("Continue with new evidence")
                        .expect("mail text"),
                    artifacts: Vec::new(),
                },
            },
        })
        .await
        .expect("append resumed work");
    owner
        .wake(WakeHint::new(
            identity,
            item("wake-after-resume"),
            TurnId::new("wake-after-resume").expect("turn"),
        ))
        .expect("wake resumed child");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(
                owner.next_update().await.expect("resumed progress"),
                OwnedRunnerUpdate::WakeScheduled { .. }
            ) {
                break;
            }
        }
    })
    .await
    .expect("resumed wake dispatches");
    let calls = resumed_driver.calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0]
            .request
            .atoms
            .iter()
            .filter(|atom| matches!(atom.value(), ContextAtomValue::Collaboration(_)))
            .count(),
        2
    );

    owner.begin_shutdown().await.expect("begin shutdown");
    loop {
        if owner
            .next_update()
            .await
            .expect("shutdown update")
            .is_finished()
        {
            break;
        }
    }
    owner.finish_shutdown().await.expect("join owner");
}

/// CMP-1/COL-4: the live file owner and canonical reopen produce the same mail snapshot.
#[tokio::test]
async fn cmp_1_owned_mail_snapshot_equals_the_reopened_projection() {
    let directory = Directory::new();
    let path = directory.0.join("owned-collaboration.jsonl");
    let writer = CollaborationWriter::spawn(two_child_collaboration(&directory))
        .expect("collaboration writer");
    let mut owner = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    for (name, from, to) in [
        ("incoming", endpoint("one"), endpoint("main")),
        ("sent", endpoint("main"), endpoint("one")),
    ] {
        owner
            .admit(CollaborationAttempt {
                id: item(name),
                event: CollaborationEvent::MailAccepted {
                    mail: MailEnvelope {
                        id: MailId::new(name).expect("mail"),
                        from,
                        to,
                        summary: CollaborationText::new(format!("{name} summary"))
                            .expect("mail text"),
                        artifacts: Vec::new(),
                    },
                },
            })
            .await
            .expect("admit mail");
    }

    let live = owner
        .mail_snapshot(endpoint("main"))
        .await
        .expect("live canonical snapshot");
    assert_eq!(live.items().len(), 2);
    assert_eq!(
        live.items()[0].direction(),
        plexmaton_agent::collaboration::MailDirection::Incoming
    );
    assert_eq!(
        live.items()[1].direction(),
        plexmaton_agent::collaboration::MailDirection::Sent
    );

    owner.begin_shutdown().await.expect("begin shutdown");
    owner.finish_shutdown().await.expect("join owner");
    let reopened = CollaborationFile::open(path).expect("reopen collaboration");
    assert_eq!(
        reopened
            .ledger()
            .project_mail(&endpoint("main"))
            .expect("reopened canonical snapshot"),
        live
    );
}
