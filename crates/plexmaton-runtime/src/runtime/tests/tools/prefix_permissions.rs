use super::*;
use plexmaton_agent::{
    PermissionMatcher, PermissionRule, PermissionRuleAction, PermissionRuleSource,
};
use plexmaton_core::PermissionScope;
use plexmaton_permission_store::ProjectPermissionStore;

fn command(source: &str) -> Arc<FakeDriver> {
    FakeDriver::new([
        Script::Events(vec![
            called(
                0,
                "prefix-call",
                "exec_command",
                serde_json::json!({"cmd":source}),
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

fn owner(workspace: &TestWorkspace, home: &TestWorkspace) -> crate::CodingSessionPermissions {
    crate::CodingSessionPermissions::new(&workspace.catalog())
        .with_project_store(ProjectPermissionStore::open(&home.0, &workspace.0).expect("store"))
        .expect("owner")
}

fn succeeded(runtime: &LiveRuntime, expected: &str) {
    assert!(runtime.agent.journal().records().iter().any(|record| matches!(record,
        plexmaton_agent::JournalRecord::AppendEntry { entry, .. } if matches!(&entry.payload,
            plexmaton_agent::JournalEntryPayload::ToolCallChanged { outcome: Some(ToolOutcome::Succeeded { output }), .. }
                if output.contains(expected)
        )
    )), "real command must produce {expected}");
}

async fn remember_project(runtime: &mut LiveRuntime, expected_prefix: &str) {
    let approval_id = next_approval(runtime).await;
    let offer = runtime
        .agent
        .pending_approvals()
        .next()
        .expect("pending")
        .permission_offer()
        .expect("offer")
        .clone();
    assert_eq!(
        offer.label,
        format!("{expected_prefix} …; same cwd/environment")
    );
    runtime
        .submit(
            agent_id(),
            Input::ApprovalDecided {
                approval_id,
                decision: ApprovalDecision::AllowAndRemember {
                    offer: offer.id,
                    scope: PermissionScope::Project,
                },
            },
        )
        .await
        .expect("remember");
    finish_active(runtime).await;
}

/// PER-10/PER-6: an offered prefix is durably saved and authorizes a different real command after restart.
#[tokio::test]
async fn per_10_project_prefix_reuses_changed_arguments_and_external_revoke_stops_dispatch() {
    let workspace = TestWorkspace::new("prefix-project");
    let home = TestWorkspace::new("prefix-home");
    std::fs::write(workspace.0.join("first"), "one").expect("file");
    std::fs::write(workspace.0.join("second name"), "two").expect("file");
    let mut first = runtime(command("ls first"), &workspace);
    first
        .use_coding_session(owner(&workspace, &home))
        .expect("owner");
    submit(&mut first, "list and remember").await;
    remember_project(&mut first, "ls").await;
    succeeded(&first, "first");
    let grants = first.coding_session().snapshot().expect("view");
    assert!(
        matches!(&grants.project_grants()[0].matcher, PermissionMatcher::CommandPrefix { prefix, .. }
        if prefix.arguments() == ["ls"])
    );
    first.shutdown().await.expect("shutdown");
    drop(first);

    let restarted = owner(&workspace, &home);
    let mut second = runtime(command("ls 'second name'"), &workspace);
    second
        .use_coding_session(restarted.clone())
        .expect("new Session");
    submit(&mut second, "reuse stored scope").await;
    let events = finish_active(&mut second).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.event, ConversationEvent::AttentionRequested { .. }))
    );
    succeeded(&second, "second name");
    second.shutdown().await.expect("shutdown");
    drop(second);

    let store = ProjectPermissionStore::open(&home.0, &workspace.0).expect("external store");
    let transaction = store.transaction(&|| false).expect("transaction");
    let view = transaction.snapshot().clone();
    drop(
        transaction
            .revoke(&view.revision, view.grants[0].id.clone(), &|| false)
            .expect("external revoke"),
    );
    let mut third = runtime(command("ls first"), &workspace);
    third.use_coding_session(restarted).expect("stale owner");
    submit(&mut third, "attempt after external revoke").await;
    finish_active(&mut third).await;
    assert!(third.agent.journal().records().iter().any(|record| matches!(record,
        plexmaton_agent::JournalRecord::AppendEntry { entry, .. } if matches!(&entry.payload,
            plexmaton_agent::JournalEntryPayload::ToolCallChanged { outcome: Some(ToolOutcome::PermissionRefused { .. }), .. })
    )));
    third.shutdown().await.expect("shutdown");
}

/// PER-10/PER-2: a remembered prefix cannot dispatch a sibling, substitution or redirected side effect.
#[tokio::test]
async fn per_10_uncovered_syntax_keeps_exact_review_and_explicit_rules_precede_reuse() {
    let workspace = TestWorkspace::new("prefix-refusal");
    let home = TestWorkspace::new("prefix-refusal-home");
    std::fs::write(workspace.0.join("first"), "one").expect("file");
    let mut first = runtime(command("ls first"), &workspace);
    first
        .use_coding_session(owner(&workspace, &home))
        .expect("owner");
    submit(&mut first, "remember").await;
    remember_project(&mut first, "ls").await;
    first.shutdown().await.expect("shutdown");
    drop(first);
    for source in [
        "ls first && printf hit > sibling",
        "ls > redirected",
        "ls \"$(printf hit > substituted)\"",
        "sh -c 'ls first'",
        "env CHANGED=1 ls first",
    ] {
        let mut pending = runtime(command(source), &workspace);
        pending
            .use_coding_session(owner(&workspace, &home))
            .expect("owner");
        submit(&mut pending, "attempt uncovered operation").await;
        next_approval(&mut pending).await;
        let approval = pending.agent.pending_approvals().next().expect("waiter");
        let offer = approval.permission_offer().expect("exact fallback");
        assert_eq!(offer.label, "exact command; same cwd/environment");
        assert!(
            offer
                .note
                .as_ref()
                .is_some_and(|note| note.contains("prefix"))
        );
        assert!(!workspace.0.join("sibling").exists());
        assert!(!workspace.0.join("redirected").exists());
        assert!(!workspace.0.join("substituted").exists());
        pending.shutdown().await.expect("cancel pending");
    }
    for action in [PermissionRuleAction::Ask, PermissionRuleAction::Deny] {
        let compiler = workspace.catalog().permission_compiler();
        let owner = owner(&workspace, &home)
            .with_user_rules(vec![PermissionRule {
                source: PermissionRuleSource::Runtime,
                action,
                matcher: compiler.command_prefix(vec!["ls".into()]).expect("scope"),
            }])
            .expect("rules");
        let mut restricted = runtime(command("ls first"), &workspace);
        restricted.use_coding_session(owner).expect("owner");
        submit(&mut restricted, "attempt explicit rule").await;
        if action == PermissionRuleAction::Ask {
            next_approval(&mut restricted).await;
            assert!(
                restricted
                    .agent
                    .pending_approvals()
                    .next()
                    .expect("waiter")
                    .permission_offer()
                    .is_none()
            );
        } else {
            finish_active(&mut restricted).await;
            assert!(restricted.agent.pending_approvals().next().is_none());
            assert!(restricted.agent.journal().records().iter().any(|record| matches!(record,
                plexmaton_agent::JournalRecord::AppendEntry { entry, .. } if matches!(&entry.payload,
                    plexmaton_agent::JournalEntryPayload::ToolCallChanged { outcome: Some(ToolOutcome::Forbidden), .. })
            )));
        }
        restricted.shutdown().await.expect("shutdown");
    }
}

/// PER-10: backend offers meaningful floors only for a complete recognized command shape.
#[tokio::test]
async fn per_10_offers_use_meaningful_floors_and_explain_exact_fallback() {
    let workspace = TestWorkspace::new("prefix-offers");
    for (source, prefix) in [
        ("ls -la src", Some("ls")),
        ("git fetch 'team origin'", Some("git fetch")),
        ("git status --short", Some("git status")),
        ("git -C ../other fetch origin", None),
        ("git --git-dir=../other fetch origin", None),
        ("python script.py", None),
        ("ls one && ls two", None),
        ("ls foo\\\nbar", None),
    ] {
        let mut pending = runtime(command(source), &workspace);
        submit(&mut pending, "review scope").await;
        next_approval(&mut pending).await;
        let offer = pending
            .agent
            .pending_approvals()
            .next()
            .expect("pending")
            .permission_offer()
            .expect("offer");
        match prefix {
            Some(prefix) => assert_eq!(offer.label, format!("{prefix} …; same cwd/environment")),
            None => assert!(
                offer
                    .label
                    .starts_with("exact command; same cwd/environment")
            ),
        }
        pending.shutdown().await.expect("cancel without execution");
    }
}
