use super::super::tools::TestWorkspace;
use super::*;
use plexmaton_core::{ApprovalDecision, PermissionScope};

async fn pending_command(
    store: ControlledStore,
    workspace: &TestWorkspace,
) -> (LiveRuntime, Input) {
    pending_command_with_scope(store, workspace, PermissionScope::Session).await
}

async fn pending_command_with_scope(
    store: ControlledStore,
    workspace: &TestWorkspace,
    scope: PermissionScope,
) -> (LiveRuntime, Input) {
    let driver = FakeDriver::new([
        Script::Events(vec![
            ModelEvent::Called {
                position: ModelOutputPosition::new(0, 0),
                call: ToolCall {
                    call_id: ToolCallId::new("permission-effect").expect("id"),
                    name: "exec_command".to_owned(),
                    arguments: serde_json::json!({"cmd":"printf hit > effect", "timeout_ms":null})
                        .to_string(),
                },
            },
            super::super::complete_usage(10, 2),
            ModelEvent::Stopped(StopReason::ToolCalls),
        ]),
        Script::Events(vec![
            super::super::complete_usage(20, 3),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
    ]);
    let clock = Arc::new(crate::runtime::clock::SystemWallClock::new().expect("clock"));
    let metadata = ConversationMetadata::new(
        ConversationId::new("permission-audit").expect("id"),
        crate::runtime::clock::WallClock::now(clock.as_ref()),
    );
    let mut runtime = LiveRuntime::with_driver_store_and_clock(
        agent_id(),
        "Plexmaton".to_owned(),
        driver,
        workspace.catalog(),
        metadata,
        Box::new(store),
        clock,
    )
    .await
    .expect("durable runtime");
    if scope == PermissionScope::Project {
        let project = plexmaton_permission_store::ProjectPermissionStore::open(
            &workspace.0.join("personal-home"),
            &workspace.0,
        )
        .expect("project store");
        let owner = crate::CodingSessionPermissions::new(&workspace.catalog())
            .with_project_store(project)
            .expect("project owner");
        runtime.use_coding_session(owner).expect("attach Project");
    }
    runtime
        .submit(agent_id(), submission())
        .await
        .expect("submit");
    let decision = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(pending) = runtime.agent.pending_approvals().next() {
                return Input::ApprovalDecided {
                    approval_id: pending.approval_id().clone(),
                    decision: ApprovalDecision::AllowAndRemember {
                        offer: pending.permission_offer().expect("offer").id,
                        scope,
                    },
                };
            }
            runtime.next_update().await.expect("drive approval");
        }
    })
    .await
    .expect("approval deadline");
    (runtime, decision)
}

async fn hold_prepared_execution(
    runtime: &mut LiveRuntime,
    control: &StoreControl,
    decision: Input,
) {
    control.block_on_payload(BlockPayload::PermissionDecision);
    runtime
        .submit(agent_id(), decision)
        .await
        .expect("prepare decision");
    tokio::time::timeout(
        Duration::from_secs(5),
        drive_until_store_blocks(runtime, control),
    )
    .await
    .expect("execution audit deadline");
    assert!(
        runtime.tools.is_empty(),
        "execution cannot precede audit acknowledgement"
    );
    let owner = runtime.coding_session();
    let snapshot = owner.snapshot().expect("prepared grant");
    assert_eq!(snapshot.grants().len() + snapshot.project_grants().len(), 1);
}

/// PER-5/PER-9/JRN-7: a failed or uncertain Conversation audit preserves an applied grant but starts no effect.
#[tokio::test]
async fn per_5_failed_remember_audit_never_dispatches_the_prepared_command() {
    for unknown in [false, true] {
        let workspace = TestWorkspace::new("permission-audit-failure");
        let (control, store) = StoreControl::pair();
        let (mut runtime, decision) = pending_command(store, &workspace).await;
        let _release = ReleaseGateOnDrop(&control.gate);
        hold_prepared_execution(&mut runtime, &control, decision).await;
        assert!(!workspace.0.join("effect").exists());
        control.fail_after(0, unknown);
        control.gate.release();
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Err(error) = runtime.next_update().await {
                    return error;
                }
            }
        })
        .await
        .expect("audit failure deadline");
        assert!(
            matches!(result, RuntimeError::JournalAppendFailed { .. }),
            "{result:?}"
        );
        assert!(
            !workspace.0.join("effect").exists(),
            "failed audit dispatched a command"
        );
        assert!(runtime.tools.is_empty(), "failure joins all workers");
        assert_eq!(
            runtime
                .coding_session()
                .snapshot()
                .expect("retained authority")
                .grants()
                .len(),
            1
        );
    }
}

/// PER-5/PER-9: revocation while the real writer holds permission provenance prevents dispatch after a successful audit.
#[tokio::test]
async fn per_5_revocation_between_preparation_and_dispatch_refuses_the_effect() {
    let workspace = TestWorkspace::new("permission-dispatch-race");
    let (control, store) = StoreControl::pair();
    let (mut runtime, decision) = pending_command(store, &workspace).await;
    let _release = ReleaseGateOnDrop(&control.gate);
    hold_prepared_execution(&mut runtime, &control, decision).await;
    let owner = runtime.coding_session();
    let view = owner.snapshot().expect("prepared grant");
    owner
        .revoke_session_grant(view.revision(), &view.grants()[0].id)
        .expect("revoke before dispatch");
    control.gate.release();
    super::super::finish_active(&mut runtime).await;
    assert!(!workspace.0.join("effect").exists());
    assert!(
        runtime
            .agent
            .journal()
            .records()
            .iter()
            .any(|record| matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(&entry.payload, JournalEntryPayload::ToolCallChanged {
                        outcome: Some(plexmaton_agent::ToolOutcome::PermissionRefused { .. }), ..
                    })
            )),
        "the model receives a typed refusal, not an abandoned call"
    );
}

/// PER-5/APV-6: interrupt accepted while the audit is blocked cancels the call before dispatch.
#[tokio::test]
async fn per_5_interrupt_before_permission_audit_ack_starts_no_worker() {
    cancellation_before_dispatch(false).await;
}

/// PER-5/APV-6: shutdown starts at acceptance and survives a cancelled audit waiter.
#[tokio::test]
async fn per_5_shutdown_before_permission_audit_ack_starts_no_worker() {
    cancellation_before_dispatch(true).await;
}

async fn cancellation_before_dispatch(shutdown: bool) {
    let workspace = TestWorkspace::new("permission-interrupt-race");
    let (control, store) = StoreControl::pair();
    let (mut runtime, decision) = pending_command(store, &workspace).await;
    let _release = ReleaseGateOnDrop(&control.gate);
    hold_prepared_execution(&mut runtime, &control, decision).await;
    {
        let interrupt = async {
            if shutdown {
                runtime.shutdown().await
            } else {
                runtime.submit(agent_id(), Input::Interrupted).await
            }
        };
        tokio::pin!(interrupt);
        tokio::select! {
            biased;
            result = &mut interrupt => panic!("interrupt crossed blocked audit: {result:?}"),
            () = tokio::task::yield_now() => {}
        }
    }
    // Hold the cancellation record too, so dispatch cannot be hidden by a fast subsequent kill.
    let _release_cancel = ReleaseGateOnDrop(&control.cancel_gate);
    control.cancel_gate.arm();
    control.gate.release();
    {
        let resume = async {
            if shutdown {
                runtime.shutdown().await.expect("resume shutdown");
            } else {
                super::super::finish_active(&mut runtime).await;
            }
        };
        tokio::pin!(resume);
        tokio::select! {
            () = &mut resume => panic!("cancellation crossed blocked audit"),
            result = tokio::time::timeout(Duration::from_secs(5), control.cancel_gate.entered.notified()) => {
                result.expect("cancellation audit deadline");
            }
        }
    }
    assert!(
        runtime.tools.is_empty(),
        "interrupt was accepted before dispatch, so no worker may start"
    );
    control.cancel_gate.release();
    if shutdown {
        runtime.shutdown().await.expect("finish shutdown");
    } else {
        super::super::finish_active(&mut runtime).await;
    }
    assert!(!workspace.0.join("effect").exists());
    assert!(runtime.tools.is_empty());
}

/// PER-6/JRN-7: the real project grant remains after a failed Conversation audit and starts no command.
#[tokio::test]
async fn per_6_project_grant_saved_then_conversation_audit_failed_starts_no_effect() {
    for unknown in [false, true] {
        let workspace = TestWorkspace::new("project-audit-failure");
        let (control, store) = StoreControl::pair();
        let (mut runtime, decision) =
            pending_command_with_scope(store, &workspace, PermissionScope::Project).await;
        let _release = ReleaseGateOnDrop(&control.gate);
        hold_prepared_execution(&mut runtime, &control, decision).await;
        control.fail_after(0, unknown);
        control.gate.release();
        let error = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Err(error) = runtime.next_update().await {
                    return error;
                }
            }
        })
        .await
        .expect("audit failure deadline");
        assert!(
            matches!(
                error,
                RuntimeError::ProjectPermissionSavedAuditFailed { .. }
            ),
            "{error:?}"
        );
        assert!(error.to_string().contains("was saved"));
        assert!(error.to_string().contains("tool did not run"));
        assert!(!workspace.0.join("effect").exists());
        assert!(runtime.tools.is_empty());
        let project = plexmaton_permission_store::ProjectPermissionStore::open(
            &workspace.0.join("personal-home"),
            &workspace.0,
        )
        .expect("reopen project");
        assert_eq!(
            project
                .read(&|| false)
                .expect("grant retained on disk")
                .grants
                .len(),
            1
        );
    }
}

/// PER-6/JRN-7: a completed Project worker receipt survives failure of an unrelated cancellation audit.
#[tokio::test]
async fn per_6_project_receipt_survives_a_different_audit_failing_before_worker_delivery() {
    for fail in [false, true] {
        let workspace = TestWorkspace::new("project-receipt-race");
        let (control, store) = StoreControl::pair();
        let (mut runtime, decision) =
            pending_command_with_scope(store, &workspace, PermissionScope::Project).await;
        runtime
            .submit(agent_id(), decision)
            .await
            .expect("start preparation");
        let owner = runtime.coding_session();
        // Do not poll runtime completion: observing the committed projection establishes readiness.
        let grant = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(grant) = owner.snapshot().expect("snapshot").project_grants().first() {
                    break grant.id.clone();
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("project acknowledgement");
        if fail {
            control.fail_after(1, true);
        }
        let result = runtime.submit(agent_id(), Input::Interrupted).await;
        let report = if fail {
            assert!(
                matches!(result, Err(RuntimeError::JournalAppendFailed { .. })),
                "{result:?}"
            );
            runtime
                .shutdown()
                .await
                .expect("shutdown returns retained receipt")
        } else {
            result.expect("interrupt")
        };
        assert_eq!(
            report.saved_project_permissions,
            vec![plexmaton_core::SavedProjectPermission {
                call_id: ToolCallId::new("permission-effect").expect("call"),
                grant
            }]
        );
        assert!(
            !workspace.0.join("effect").exists(),
            "an undelivered preparation cannot run a tool"
        );
        assert!(runtime.tools.is_empty());
        let store = plexmaton_permission_store::ProjectPermissionStore::open(
            &workspace.0.join("personal-home"),
            &workspace.0,
        )
        .expect("reopen");
        assert_eq!(
            store.read(&|| false).expect("grant retained").grants.len(),
            1
        );
        if !fail {
            runtime.shutdown().await.expect("shutdown");
        }
    }
}
