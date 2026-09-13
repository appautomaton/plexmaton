use super::*;
use crate::runtime::clock::WallClock;
use crate::runtime::tests::text_delta;
use crate::{TreeAdmission, TreeRequestRefusal};
use plexmaton_agent::{
    Agent, ApprovalPolicy, ConversationMetadata, Input, JournalEntryPayload, ModelEvent,
    ModelOutputPosition, SkillActivation, SkillSource, StopReason, TurnBudget, UnixMillis,
};
use plexmaton_core::{
    AgentId, ConversationEntryId, ConversationId, TreeNavigation, TreeNavigationTarget,
    TreeRevision,
};
use plexmaton_session_store::JournalRecovery;

const HISTORICAL_TEXT: &str = "  $73 inspect\r\nexactly  ";

pub(super) async fn seeded_runtime(
    driver: Arc<FakeDriver>,
) -> (
    StoreControl,
    LiveRuntime,
    Vec<ConversationEntryId>,
    ConversationEntryId,
    plexmaton_core::HeadName,
) {
    let agent_id = agent_id();
    let clock = Arc::new(crate::runtime::clock::FixedWallClock(UnixMillis::new(20)));
    let metadata = ConversationMetadata::new(
        ConversationId::new("session-durable").expect("conversation id"),
        clock.now(),
    );
    let mut agent = Agent::for_conversation(
        agent_id.clone(),
        metadata,
        TurnBudget::default(),
        ApprovalPolicy::default(),
    );
    let _announcement = agent.announce("Plexmaton");
    let skill = SkillActivation::new(
        "73".to_owned(),
        SkillSource::ProjectShared,
        "/fixture/.agents/skills/73/SKILL.md".to_owned(),
        "c".repeat(64),
        "historical instructions\r\n".to_owned(),
    )
    .expect("historical skill");
    let started = agent.handle_at(
        Input::SkillSubmitted {
            text: HISTORICAL_TEXT.to_owned(),
            skill,
        },
        clock.now(),
    );
    assert_eq!(started.effects.len(), 1, "seed opens one model step");
    let step_id = agent.active_model_step().expect("seed step id");
    let _partial = agent.handle_at(
        Input::Streamed {
            step_id: step_id.clone(),
            event: text_delta("historical answer"),
        },
        clock.now(),
    );
    let _finished = agent.handle_at(
        Input::Streamed {
            step_id,
            event: ModelEvent::Stopped(StopReason::EndOfTurn),
        },
        clock.now(),
    );
    assert!(!agent.is_running(), "seed turn is complete");

    let source_head = agent.selected_head().clone();
    let source_path = agent
        .journal()
        .path(&source_head)
        .expect("main path")
        .into_iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    let target = agent
        .journal()
        .path(&source_head)
        .expect("main path")
        .into_iter()
        .find_map(|entry| {
            matches!(entry.payload, JournalEntryPayload::TurnStarted { .. })
                .then(|| entry.id.clone())
        })
        .expect("historical user entry");
    let existing_records = agent.journal().records().to_vec();

    let (control, store) = StoreControl::pair();
    *control
        .records
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = existing_records;
    let workspace = std::env::current_dir().expect("workspace path");
    let tools = NativeToolCatalog::open(
        &workspace,
        "TEST_KEY",
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .expect("test tools");
    let (runtime, recovery) = LiveRuntime::with_resumed_driver_and_store(
        agent_id,
        agent,
        driver,
        tools,
        Box::new(store),
        JournalRecovery::Clean,
        clock,
    )
    .await
    .expect("resume seeded runtime");
    assert!(!recovery.interrupted_turn, "seed is a settled conversation");
    (control, runtime, source_path, target, source_head)
}

fn rewind(origin: plexmaton_core::TreeOrigin, target: ConversationEntryId) -> TreeNavigation {
    TreeNavigation {
        origin,
        target: TreeNavigationTarget::Rewind(target),
    }
}

fn store_records(control: &StoreControl) -> Vec<JournalRecord> {
    control
        .records
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// TRE-4: a staged navigation stays hidden until acknowledgement; a cancelled waiter does not
/// abandon it, and its receipt cannot be overwritten before the caller consumes the report.
#[tokio::test]
async fn tre_4_navigation_is_acknowledgement_gated_cancellation_safe_and_report_owned() {
    let driver = FakeDriver::new([Script::Events(vec![
        text_delta("resubmitted answer"),
        ModelEvent::Stopped(StopReason::EndOfTurn),
    ])]);
    let (control, mut runtime, source_path, target, source_head) =
        seeded_runtime(Arc::clone(&driver)).await;
    while runtime.try_next_event().is_some() {}
    let origin = runtime
        .acknowledged_tree_origin()
        .expect("idle seeded origin is acknowledged");
    let source_records = store_records(&control);
    let _release = ReleaseGateOnDrop(&control.gate);
    control.block_after(1);

    assert!(
        matches!(
            runtime.request_tree_navigation(rewind(origin, target.clone())),
            Ok(TreeAdmission::Started)
        ),
        "admission owns the append without waiting for it"
    );
    {
        let entered = control.gate.entered.notified();
        let update = runtime.next_update();
        tokio::pin!(update);
        tokio::select! {
            result = &mut update => panic!("navigation became visible before acknowledgement: {result:?}"),
            () = entered => {}
        }
    }
    assert_eq!(runtime.acknowledged_tree_origin(), None);
    assert!(runtime.report.projection_reset.is_none());
    assert!(runtime.report.tree_navigation.is_none());
    assert!(runtime.report.undelivered.is_empty());
    assert!(runtime.try_next_event().is_none());
    assert!(
        driver.calls().await.is_empty(),
        "navigation never starts a model"
    );

    control.gate.release();
    runtime
        .finish_transition()
        .await
        .expect("cancelled waiter resumes owned navigation");
    let new_origin = runtime
        .acknowledged_tree_origin()
        .expect("origin becomes available after acknowledgement");
    assert!(matches!(
        runtime.request_tree_navigation(TreeNavigation {
            origin: new_origin.clone(),
            target: TreeNavigationTarget::SelectHead(new_origin.selected_head.clone()),
        }),
        Ok(TreeAdmission::Refused(TreeRequestRefusal::PendingReport))
    ));
    assert!(runtime.report.tree_navigation.is_some());
    assert_eq!(
        runtime.report.projection_reset.as_ref().map(Vec::len),
        Some(1)
    );
    assert_eq!(store_records(&control).len(), source_records.len() + 1);

    let report = runtime.take_report();
    assert_eq!(report.persistence_failure, None);
    assert!(report.undelivered.is_empty());
    assert_eq!(report.projection_reset.as_ref().map(Vec::len), Some(1));
    let receipt = report.tree_navigation.expect("acknowledged receipt");
    assert!(receipt.mutation_sequence.is_some());
    assert_ne!(receipt.selected_head, source_head);
    assert_eq!(
        receipt.returned_draft,
        Some(plexmaton_agent::ReturnedDraft {
            text: HISTORICAL_TEXT.to_owned(),
            skill_name: Some("73".to_owned()),
        })
    );
    assert_eq!(
        runtime
            .agent
            .journal()
            .path(&source_head)
            .expect("source main path")
            .into_iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
        source_path,
        "rewind leaves the source path untouched"
    );
    assert!(runtime.try_next_event().is_none());
    assert!(
        driver.calls().await.is_empty(),
        "returned draft is not submitted"
    );

    let attempts_after_commit = control.attempts.load(Ordering::SeqCst);
    let selected_origin = runtime
        .acknowledged_tree_origin()
        .expect("new selected origin remains acknowledged");
    assert!(
        matches!(
            runtime.request_tree_navigation(TreeNavigation {
                origin: selected_origin.clone(),
                target: TreeNavigationTarget::SelectHead(selected_origin.selected_head.clone()),
            }),
            Ok(TreeAdmission::NoOp)
        ),
        "selecting the current head does not write"
    );
    assert_eq!(
        control.attempts.load(Ordering::SeqCst),
        attempts_after_commit
    );
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: receipt.returned_draft.expect("returned draft").text,
            },
        )
        .await
        .expect("resubmission follows the ordinary submit boundary");
    assert_eq!(driver.calls().await.len(), 1);
}

/// TRE-4: a pending approval makes navigation busy even when there is no provider/tool worker;
/// refusing it must preserve the approval and leave the journal unchanged.
#[tokio::test]
async fn tre_4_pending_approval_refuses_navigation_without_flushing_or_writing() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::Events(vec![
        ModelEvent::Called {
            position: ModelOutputPosition::new(0, 0),
            call: ToolCall {
                call_id: ToolCallId::new("navigation-approval-call").expect("call id"),
                name: "exec_command".to_owned(),
                arguments: serde_json::json!({"cmd":"false","timeout_ms":null}).to_string(),
            },
        },
        ModelEvent::Stopped(StopReason::ToolCalls),
    ])]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    while runtime.try_next_event().is_some() {}
    runtime
        .submit(agent_id(), submission())
        .await
        .expect("open approval turn");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if runtime.agent.pending_approvals().next().is_some() {
                break;
            }
            match runtime.next_update().await.expect("drive approval") {
                RuntimeUpdate::Event(_) | RuntimeUpdate::Report(_) => {}
                RuntimeUpdate::Finished => panic!("runtime finished before approval"),
            }
        }
    })
    .await
    .expect("approval deadline");

    let origin = runtime
        .acknowledged_tree_origin()
        .expect("pending approval still has an acknowledged origin");
    let prior_records = store_records(&control);
    let attempts = control.attempts.load(Ordering::SeqCst);
    assert!(
        !runtime.has_active_work(),
        "approval is outside active workers"
    );
    assert!(matches!(
        runtime.request_tree_navigation(rewind(
            origin,
            ConversationEntryId::new("missing-navigation-target").expect("entry id"),
        )),
        Ok(TreeAdmission::Refused(TreeRequestRefusal::Busy))
    ));
    assert!(runtime.agent.is_running());
    assert_eq!(runtime.agent.pending_approvals().count(), 1);
    assert_eq!(store_records(&control), prior_records);
    assert_eq!(control.attempts.load(Ordering::SeqCst), attempts);
    assert_eq!(driver.calls().await.len(), 1);
    assert!(runtime.report.tree_navigation.is_none());
    assert!(runtime.report.projection_reset.is_none());
}

/// TRE-4: runtime-owned queued agent input blocks navigation without being flushed or cancelled.
#[tokio::test]
async fn tre_4_queued_agent_input_refuses_navigation_and_remains_owned() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::WaitForCancellation {
        started: Arc::new(Notify::new()),
        finished: Arc::new(AtomicBool::new(false)),
    }]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    while runtime.try_next_event().is_some() {}
    runtime
        .submit(agent_id(), submission())
        .await
        .expect("open first turn");
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "queued behind active turn".to_owned(),
            },
        )
        .await
        .expect("queue second turn");
    let origin = runtime
        .acknowledged_tree_origin()
        .expect("queued input has an acknowledged tree origin");
    let target = runtime
        .agent
        .journal()
        .path(runtime.agent.selected_head())
        .expect("open path")
        .into_iter()
        .find_map(|entry| {
            matches!(entry.payload, JournalEntryPayload::TurnStarted { .. })
                .then(|| entry.id.clone())
        })
        .expect("first user entry");
    let prior_records = store_records(&control);
    let attempts = control.attempts.load(Ordering::SeqCst);
    assert!(matches!(
        runtime.request_tree_navigation(rewind(origin, target)),
        Ok(TreeAdmission::Refused(TreeRequestRefusal::Busy))
    ));
    assert!(runtime.agent.is_running());
    assert_eq!(
        runtime.agent.queued_for_next_turn().collect::<Vec<_>>(),
        ["queued behind active turn"]
    );
    assert!(runtime.has_active_model());
    assert_eq!(store_records(&control), prior_records);
    assert_eq!(control.attempts.load(Ordering::SeqCst), attempts);
    assert_eq!(driver.calls().await.len(), 1);
    assert!(runtime.report.tree_navigation.is_none());
    assert!(runtime.report.projection_reset.is_none());
}

/// TRE-4: stale/foreign origins and missing targets are typed refusals and enqueue no mutation.
#[tokio::test]
async fn tre_4_stale_and_invalid_navigation_refuse_without_writing() {
    let driver = FakeDriver::new([]);
    let (control, mut runtime, _, target, _) = seeded_runtime(driver).await;
    while runtime.try_next_event().is_some() {}
    let origin = runtime
        .acknowledged_tree_origin()
        .expect("seeded origin is acknowledged");
    let baseline = store_records(&control);
    let attempts = control.attempts.load(Ordering::SeqCst);
    let foreign_agent = AgentId::new("agent-foreign").expect("foreign agent id");
    let mut foreign = origin.clone();
    foreign.agent_id = foreign_agent;
    assert!(matches!(
        runtime.request_tree_navigation(rewind(foreign, target.clone())),
        Ok(TreeAdmission::Refused(TreeRequestRefusal::Navigation(
            plexmaton_agent::TreeNavigationRefusal::ForeignAgent { .. }
        )))
    ));

    let mut stale = origin.clone();
    stale.revision = TreeRevision::new(origin.revision.get() + 1);
    assert!(matches!(
        runtime.request_tree_navigation(rewind(stale, target.clone())),
        Ok(TreeAdmission::Refused(
            TreeRequestRefusal::StaleOrigin { .. }
        ))
    ));

    assert!(matches!(
        runtime.request_tree_navigation(rewind(
            origin,
            ConversationEntryId::new("missing-navigation-target").expect("entry id"),
        )),
        Ok(TreeAdmission::Refused(TreeRequestRefusal::Navigation(
            plexmaton_agent::TreeNavigationRefusal::MissingTarget(_)
        )))
    ));
    assert_eq!(store_records(&control), baseline);
    assert_eq!(control.attempts.load(Ordering::SeqCst), attempts);
    assert!(runtime.report.tree_navigation.is_none());
    assert!(runtime.report.projection_reset.is_none());
}

/// TRE-4/JRN-7: definite and uncertain append failures publish no navigation result or fake input;
/// the runtime freezes until reopen rather than exposing its staged in-memory destination.
#[tokio::test]
async fn tre_4_navigation_write_failures_freeze_without_result_or_undelivered_input() {
    for unknown in [false, true] {
        let driver = FakeDriver::new([]);
        let (control, mut runtime, _, target, _) = seeded_runtime(driver.clone()).await;
        while runtime.try_next_event().is_some() {}
        let origin = runtime
            .acknowledged_tree_origin()
            .expect("seeded origin is acknowledged");
        let baseline = store_records(&control);
        control.fail_after(1, unknown);
        assert!(matches!(
            runtime.request_tree_navigation(rewind(origin.clone(), target)),
            Ok(TreeAdmission::Started)
        ));
        let report = await_report(&mut runtime).await;
        assert_eq!(
            report.persistence_failure,
            Some(if unknown {
                PersistenceFailure::OutcomeUnknown
            } else {
                PersistenceFailure::NotWritten
            })
        );
        assert!(report.undelivered.is_empty());
        assert!(report.tree_navigation.is_none());
        assert!(report.projection_reset.is_none());
        assert_eq!(runtime.acknowledged_tree_origin(), None);
        assert_eq!(store_records(&control), baseline);
        assert!(driver.calls().await.is_empty());
        assert!(matches!(
            runtime.request_tree_navigation(rewind(
                origin,
                ConversationEntryId::new("missing-navigation-target").expect("entry id"),
            )),
            Ok(TreeAdmission::Refused(
                TreeRequestRefusal::PersistenceFailed
            ))
        ));
    }
}

/// TRE-4/JRN-7: shutdown after admission joins the accepted append and returns its receipt rather
/// than claiming rollback; later requests are refused once shutdown owns the runtime.
#[tokio::test]
async fn tre_4_shutdown_joins_an_accepted_navigation_append() {
    let driver = FakeDriver::new([]);
    let (control, mut runtime, _, target, _) = seeded_runtime(driver.clone()).await;
    while runtime.try_next_event().is_some() {}
    let origin = runtime
        .acknowledged_tree_origin()
        .expect("seeded origin is acknowledged");
    let _release = ReleaseGateOnDrop(&control.gate);
    control.block_after(1);
    assert!(matches!(
        runtime.request_tree_navigation(rewind(origin.clone(), target)),
        Ok(TreeAdmission::Started)
    ));

    {
        let entered = control.gate.entered.notified();
        let shutdown = runtime.shutdown();
        tokio::pin!(shutdown);
        tokio::select! {
            biased;
            result = &mut shutdown => panic!("shutdown passed the blocked append: {result:?}"),
            () = entered => {}
        }
    }
    // Cancellation drops only the waiter; shutdown and its accepted append remain runtime-owned.
    assert_eq!(runtime.acknowledged_tree_origin(), None);
    assert!(matches!(
        runtime.request_tree_navigation(rewind(
            origin,
            ConversationEntryId::new("missing-navigation-target").expect("entry id"),
        )),
        Ok(TreeAdmission::Refused(TreeRequestRefusal::ShuttingDown))
    ));
    control.gate.release();
    let report = runtime
        .shutdown()
        .await
        .expect("shutdown joins accepted append");
    assert!(report.tree_navigation.is_some());
    assert!(report.projection_reset.is_some());
    assert!(report.undelivered.is_empty());
    assert!(driver.calls().await.is_empty());
    assert!(control.dropped.load(Ordering::SeqCst));
}
