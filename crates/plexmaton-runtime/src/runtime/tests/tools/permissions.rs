use super::*;
use plexmaton_core::PermissionScope;

pub(super) fn command_driver() -> Arc<FakeDriver> {
    FakeDriver::new([
        Script::Events(vec![
            called(
                0,
                "reused-call-id",
                "exec_command",
                serde_json::json!({"cmd": "printf hit >> hits", "timeout_ms": null}),
            ),
            complete_usage(10, 2),
            ModelEvent::Stopped(StopReason::ToolCalls),
        ]),
        Script::Events(vec![
            complete_usage(20, 3),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]),
    ])
}

/// PER-3/PER-7: enabling the Session setting releases a real native create while a shell call still waits.
#[tokio::test]
async fn per_7_session_setting_releases_native_waiters_but_leaves_commands_pending() {
    let workspace = TestWorkspace::new("native-setting");
    let driver = FakeDriver::new([Script::Events(vec![
        called(
            0,
            "native-create",
            "create_file",
            serde_json::json!({"path":"created.txt", "content":"created by native tool"}),
        ),
        called(
            1,
            "shell-write",
            "exec_command",
            serde_json::json!({"cmd":"printf no > forbidden-shell-effect", "timeout_ms":null}),
        ),
        complete_usage(10, 2),
        ModelEvent::Stopped(StopReason::ToolCalls),
    ])]);
    let mut runtime = runtime(driver, &workspace);
    // PER-7 proves that enabling the setting releases what waited, so it must start off.
    crate::runtime::tests::ask_about_file_changes(&runtime.coding_session());
    submit(&mut runtime, "create the file").await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.agent.pending_approvals().count() < 2 {
            runtime.next_update().await.expect("wait for admission");
        }
    })
    .await
    .expect("both protected calls wait");
    let owner = runtime.coding_session();
    owner
        .apply_control(
            &plexmaton_core::PermissionIntent {
                expected: owner.snapshot().expect("current view").revision().clone(),
                action: plexmaton_core::PermissionAction::EnableNativeFiles,
            },
            &|| false,
        )
        .expect("enable Session setting");
    runtime
        .permissions_changed()
        .await
        .expect("re-evaluate waiting calls");
    tokio::time::timeout(Duration::from_secs(5), async {
        while !runtime.tools.is_empty() {
            runtime.next_update().await.expect("native completion");
        }
    })
    .await
    .expect("native create effect");
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("created.txt")).expect("native output"),
        "created by native tool"
    );
    let pending: Vec<_> = runtime.agent.pending_approvals().collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].admitted().requested().call_id.as_str(),
        "shell-write"
    );
    assert!(!workspace.0.join("forbidden-shell-effect").exists());
    runtime
        .shutdown()
        .await
        .expect("cancel remaining shell approval");
}

/// PER-1/PER-4: the real executor reuses a Session grant in a replacement Conversation, then asks after revocation.
#[tokio::test]
async fn per_1_remembered_command_survives_runtime_replacement_and_revocation_restores_asking() {
    let workspace = TestWorkspace::new("permission-lifetime");
    let mut first = runtime(command_driver(), &workspace);
    submit(&mut first, "run the command").await;
    let approval = next_approval(&mut first).await;
    let offer = first
        .agent
        .pending_approvals()
        .next()
        .expect("pending")
        .permission_offer()
        .expect("backend offer")
        .clone();
    assert_eq!(offer.scopes, plexmaton_core::PermissionScopes::Session);
    let report = first
        .submit(
            agent_id(),
            Input::ApprovalDecided {
                approval_id: approval.clone(),
                decision: ApprovalDecision::AllowAndRemember {
                    offer: offer.id,
                    scope: PermissionScope::Session,
                },
            },
        )
        .await
        .expect("remember decision");
    assert!(report.unresolved_approvals.is_empty());
    finish_active(&mut first).await;
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("hits")).expect("first effect"),
        "hit"
    );
    let owner = first.coding_session();
    let granted = owner.snapshot().expect("granted view");
    assert_eq!(crate::runtime::tests::approved(granted.grants()), 1);
    assert!(
        first
            .shutdown()
            .await
            .expect("shutdown")
            .cleanup_failures
            .is_empty()
    );
    drop(first);

    let mut replacement = runtime(command_driver(), &workspace);
    replacement
        .use_coding_session(owner.clone())
        .expect("same coding Session");
    submit(&mut replacement, "same command in a new conversation").await;
    let events = finish_active(&mut replacement).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.event, ConversationEvent::AttentionRequested { .. })),
        "the remembered exact command must actually run without another question"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("hits")).expect("second effect"),
        "hithit"
    );
    assert!(
        replacement
            .shutdown()
            .await
            .expect("shutdown")
            .cleanup_failures
            .is_empty()
    );
    drop(replacement);

    owner
        .revoke_session_grant(
            granted.revision(),
            &crate::runtime::tests::approved_grant(granted.grants()).id,
        )
        .expect("revoke exact grant");
    let mut after_revoke = runtime(command_driver(), &workspace);
    after_revoke
        .use_coding_session(owner)
        .expect("same Session after revocation");
    submit(&mut after_revoke, "same command after revoke").await;
    let new_approval = next_approval(&mut after_revoke).await;
    assert_ne!(
        new_approval, approval,
        "an old one-call approval cannot match the replacement Conversation"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("hits")).expect("no third effect"),
        "hithit"
    );
    assert!(
        after_revoke
            .shutdown()
            .await
            .expect("cancel pending request")
            .cleanup_failures
            .is_empty()
    );
}

/// CMD-7/PER-2: a Session on a host that can fence a command runs one without asking.
///
/// Built through `LiveRuntime::with_driver` rather than this module's `runtime` helper, because
/// that helper revokes the preset for the fixtures that need a call which waits. This is the
/// production path, and the claim is that it is quiet.
#[tokio::test]
async fn cmd_7_a_confined_command_runs_without_asking() {
    let workspace = TestWorkspace::new("confined-quiet");
    let mut runtime = LiveRuntime::with_driver(
        agent_id(),
        "Plexmaton".to_owned(),
        command_driver(),
        workspace.catalog(),
    )
    .unwrap_or_else(|error| panic!("construct runtime: {error}"));
    if plexmaton_command::Confinement::unavailable().is_some() {
        // No fence on this host, so nothing was seeded and the question keeps its meaning. The
        // pair below is what proves the preset does the work; here there is no preset.
        return;
    }
    submit(&mut runtime, "run it").await;
    finish_active(&mut runtime).await;
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("hits")).expect("the command ran"),
        "hit"
    );
    assert_eq!(
        runtime.agent.pending_approvals().count(),
        0,
        "a fenced command is not asked about"
    );
    assert_eq!(
        crate::runtime::tests::approved(
            runtime.coding_session().snapshot().expect("view").grants()
        ),
        0,
        "and it ran on the seeded preset, not on a grant a decision created"
    );
}

/// CMD-7/PER-7: revoking the preset restores the question, which is what makes it the preset's.
#[tokio::test]
async fn cmd_7_revoking_the_preset_brings_the_question_back() {
    let workspace = TestWorkspace::new("confined-revoked");
    let mut runtime = LiveRuntime::with_driver(
        agent_id(),
        "Plexmaton".to_owned(),
        command_driver(),
        workspace.catalog(),
    )
    .unwrap_or_else(|error| panic!("construct runtime: {error}"));
    if plexmaton_command::Confinement::unavailable().is_some() {
        return;
    }
    crate::runtime::tests::ask_about_commands(&runtime.coding_session());
    submit(&mut runtime, "run it").await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.agent.pending_approvals().count() == 0 {
            runtime.next_update().await.expect("wait for admission");
        }
    })
    .await
    .expect("the revoked preset restores the question");
    assert!(
        !workspace.0.join("hits").exists(),
        "nothing runs while it waits"
    );
    runtime.shutdown().await.expect("cancel the waiting call");
}

/// PER-3/PER-7: the seeded file preset writes the project and still asks for the control plane.
///
/// The exclusion is the reason this is the preset rather than a policy default: an edit reaches
/// only what WFS-1 and MUT-2 pin, so the question buys nothing inside the project, while the paths
/// that configure the agent are exactly where it still buys something.
#[tokio::test]
async fn per_3_a_seeded_preset_writes_the_project_and_asks_for_the_control_plane() {
    let workspace = TestWorkspace::new("preset-exclusions");
    let driver = FakeDriver::new([Script::Events(vec![
        called(
            0,
            "project-file",
            "create_file",
            serde_json::json!({"path": "note.txt", "content": "ordinary"}),
        ),
        called(
            1,
            "control-file",
            "create_file",
            serde_json::json!({"path": ".agents/note.md", "content": "instructions"}),
        ),
        complete_usage(10, 2),
        ModelEvent::Stopped(StopReason::ToolCalls),
    ])]);
    std::fs::create_dir_all(workspace.0.join(".agents")).expect("control directory");
    let mut runtime = LiveRuntime::with_driver(
        agent_id(),
        "Plexmaton".to_owned(),
        driver,
        workspace.catalog(),
    )
    .unwrap_or_else(|error| panic!("construct runtime: {error}"));
    submit(&mut runtime, "write both").await;
    // Both conditions, because an approval can appear before the allowed sibling has finished.
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.agent.pending_approvals().count() == 0
            || !workspace.0.join("note.txt").exists()
        {
            runtime.next_update().await.expect("drive both calls");
        }
    })
    .await
    .expect("the ordinary write runs and the control-plane write waits");
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("note.txt")).expect("project file"),
        "ordinary",
        "an ordinary project write runs on the seeded preset"
    );
    let pending: Vec<_> = runtime.agent.pending_approvals().collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].admitted().requested().call_id.as_str(),
        "control-file",
        "the preset excludes agent-control paths at any depth"
    );
    assert!(!workspace.0.join(".agents/note.md").exists());
    drop(pending);
    runtime.shutdown().await.expect("cancel the waiting write");
}
