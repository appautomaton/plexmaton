use super::*;
use plexmaton_core::PermissionScope;
use plexmaton_permission_store::ProjectPermissionStore;

fn project_owner(
    workspace: &TestWorkspace,
    home: &TestWorkspace,
) -> crate::CodingSessionPermissions {
    let store = ProjectPermissionStore::open(&home.0, &workspace.0).expect("project store");
    crate::CodingSessionPermissions::new(&workspace.catalog())
        .with_project_store(store)
        .expect("project owner")
}

/// PER-6/PGR-2: a fresh coding Session reuses the actual stored command; a stale live owner observes revoke at dispatch.
#[tokio::test]
async fn per_6_project_command_survives_restart_and_dispatch_observes_external_revoke() {
    let workspace = TestWorkspace::new("project-restart");
    let home = TestWorkspace::new("project-home");
    let mut first = runtime(permissions::command_driver(), &workspace);
    first
        .use_coding_session(project_owner(&workspace, &home))
        .expect("attach");
    submit(&mut first, "run and remember").await;
    let approval_id = next_approval(&mut first).await;
    let offer = first
        .agent
        .pending_approvals()
        .next()
        .expect("pending")
        .permission_offer()
        .expect("offer")
        .clone();
    assert_eq!(
        offer.scopes,
        plexmaton_core::PermissionScopes::SessionAndProject
    );
    first
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
        .expect("remember project");
    finish_active(&mut first).await;
    let snapshot = first.coding_session().snapshot().expect("view");
    assert!(
        snapshot.grants().is_empty(),
        "Project is not a Session grant copy"
    );
    assert_eq!(snapshot.project_grants().len(), 1);
    first.shutdown().await.expect("shutdown");
    drop(first);

    let owner = project_owner(&workspace, &home);
    let mut second = runtime(permissions::command_driver(), &workspace);
    second
        .use_coding_session(owner.clone())
        .expect("fresh Session");
    submit(&mut second, "use stored permission").await;
    let events = finish_active(&mut second).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.event, ConversationEvent::AttentionRequested { .. }))
    );
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("hits")).expect("two effects"),
        "hithit"
    );
    second.shutdown().await.expect("shutdown");
    drop(second);

    // Independent adapter represents another process. The live owner deliberately keeps an old view.
    let store = ProjectPermissionStore::open(&home.0, &workspace.0).expect("external store");
    let transaction = store.transaction(&|| false).expect("revoke transaction");
    let reviewed = transaction.snapshot().clone();
    drop(
        transaction
            .revoke(&reviewed.revision, reviewed.grants[0].id.clone(), &|| false)
            .expect("external revoke"),
    );
    assert_eq!(
        owner.snapshot().expect("stale view").project_grants().len(),
        1
    );
    let mut after_revoke = runtime(permissions::command_driver(), &workspace);
    after_revoke
        .use_coding_session(owner)
        .expect("stale Session");
    submit(&mut after_revoke, "attempt with stale permission").await;
    finish_active(&mut after_revoke).await;
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("hits")).expect("no third effect"),
        "hithit"
    );
    assert!(after_revoke.agent.journal().records().iter().any(|record| matches!(record, plexmaton_agent::JournalRecord::AppendEntry { entry, .. } if matches!(entry.payload, plexmaton_agent::JournalEntryPayload::ToolCallChanged { outcome: Some(ToolOutcome::PermissionRefused { .. }), .. }))));
    assert!(
        after_revoke
            .coding_session()
            .snapshot()
            .expect("refreshed")
            .project_grants()
            .is_empty()
    );
}

/// PER-6/PGR-3: even Allow once cannot proceed when the required personal source becomes corrupt.
#[tokio::test]
async fn per_6_corrupt_project_source_refuses_allow_once_before_its_effect() {
    let workspace = TestWorkspace::new("project-corrupt");
    let home = TestWorkspace::new("project-corrupt-home");
    let store = ProjectPermissionStore::open(&home.0, &workspace.0).expect("store");
    drop(
        store
            .transaction(&|| false)
            .expect("transaction")
            .reset(&plexmaton_core::ProjectPermissionRevision::Absent, &|| {
                false
            })
            .expect("initialize"),
    );
    let mut runtime = runtime(permissions::command_driver(), &workspace);
    runtime
        .use_coding_session(project_owner(&workspace, &home))
        .expect("attach");
    submit(&mut runtime, "ask for command").await;
    let approval = next_approval(&mut runtime).await;
    let log = home
        .0
        .join("projects")
        .join(store.project().key())
        .join("permissions.jsonl");
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .expect("source")
        .write_all(b"{\"sequence\":1")
        .expect("torn mutation");
    allow_once(&mut runtime, approval).await;
    finish_active(&mut runtime).await;
    assert!(!workspace.0.join("hits").exists());
    assert!(matches!(
        runtime
            .coding_session()
            .snapshot()
            .expect("unavailable view")
            .project(),
        plexmaton_agent::ProjectPermissions::Unavailable
    ));
}

struct ConfigSource(std::sync::Mutex<Option<plexmaton_agent::PermissionConfiguration>>);

impl crate::ProjectPermissionConfigurationSource for ConfigSource {
    fn load(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<
        Option<plexmaton_agent::PermissionConfiguration>,
        plexmaton_core::PermissionChangeError,
    > {
        if cancelled() {
            return Err(plexmaton_core::PermissionChangeError::Unavailable);
        }
        Ok(self.0.lock().expect("source fixture").clone())
    }
}

fn configured_command(
    workspace: &TestWorkspace,
    fingerprint: [u8; 32],
) -> plexmaton_agent::PermissionConfiguration {
    plexmaton_agent::PermissionConfiguration::new(
        fingerprint,
        vec![plexmaton_agent::PermissionRule {
            source: plexmaton_agent::PermissionRuleSource::Runtime,
            action: plexmaton_agent::PermissionRuleAction::Allow,
            matcher: workspace
                .catalog()
                .permission_compiler()
                .exact_command("printf hit >> hits")
                .expect("exact native scope"),
        }],
    )
    .expect("bounded configuration")
}

/// PER-8/PER-5: configured authority actually releases a waiter; changed bytes refuse a stale Policy dispatch.
#[tokio::test]
async fn per_8_trusted_configuration_runs_the_command_and_dispatch_rechecks_changed_bytes() {
    use plexmaton_core::{PermissionAction, PermissionIntent};
    let workspace = TestWorkspace::new("configured-command");
    let home = TestWorkspace::new("configured-command-home");
    let source = Arc::new(ConfigSource(std::sync::Mutex::new(Some(
        configured_command(&workspace, [1; 32]),
    ))));
    let owner = project_owner(&workspace, &home)
        .with_project_configuration(source.clone())
        .expect("configured owner");
    let mut first = runtime(permissions::command_driver(), &workspace);
    first.use_coding_session(owner.clone()).expect("attach");
    submit(&mut first, "untrusted config asks").await;
    next_approval(&mut first).await;
    let view = owner.snapshot().expect("view").control_view();
    let intent = PermissionIntent {
        expected: view.revision,
        action: PermissionAction::TrustProjectConfiguration([1; 32]),
    };
    assert_eq!(
        owner.apply_control(&intent, &|| true),
        Err(plexmaton_core::PermissionChangeError::Unavailable)
    );
    owner
        .refresh(&|| false)
        .expect("recover cancelled observation");
    let intent = PermissionIntent {
        expected: owner.snapshot().expect("view").revision().clone(),
        ..intent
    };
    owner
        .apply_control(&intent, &|| false)
        .expect("explicit trust");
    first.permissions_changed().await.expect("recheck waiter");
    finish_active(&mut first).await;
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("hits")).expect("first effect"),
        "hit"
    );
    first.shutdown().await.expect("shutdown");

    let restarted = project_owner(&workspace, &home)
        .with_project_configuration(source.clone())
        .expect("fresh coding Session");
    let mut second = runtime(permissions::command_driver(), &workspace);
    second
        .use_coding_session(restarted.clone())
        .expect("attach");
    submit(&mut second, "trusted after restart").await;
    finish_active(&mut second).await;
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("hits")).expect("second effect"),
        "hithit"
    );
    second.shutdown().await.expect("shutdown");

    *source.0.lock().expect("edit source") = Some(configured_command(&workspace, [2; 32]));
    // The live Session still has the old immutable Allow view when the agent assembles its effect.
    assert!(
        restarted
            .snapshot()
            .expect("old view")
            .control_view()
            .configuration
            .expect("config")
            .trusted
    );
    let mut third = runtime(permissions::command_driver(), &workspace);
    third
        .use_coding_session(restarted)
        .expect("attach old projection");
    submit(&mut third, "configuration edited").await;
    finish_active(&mut third).await;
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("hits")).expect("no third effect"),
        "hithit"
    );
    assert!(
        third
            .agent
            .journal()
            .records()
            .iter()
            .any(|record| matches!(record,
                plexmaton_agent::JournalRecord::AppendEntry { entry, .. }
                if matches!(entry.payload, plexmaton_agent::JournalEntryPayload::ToolCallChanged {
                    outcome: Some(ToolOutcome::PermissionRefused { .. }), ..
                })
            ))
    );

    assert!(
        !third
            .coding_session()
            .snapshot()
            .expect("current view")
            .control_view()
            .configuration
            .expect("config")
            .trusted
    );
    third.shutdown().await.expect("shutdown");
}
