use super::*;
use crate::tests::FixtureWorkspace;
use plexmaton_agent::{Agent, ApprovalPolicy, ConversationMetadata, TurnBudget, UnixMillis};
use plexmaton_session_store::JournalFile;

const CONFIG: &str = r#"
active_model = { provider = "fixture", model = "test" }
[providers.fixture]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "PLEXMATON_PICKER_TEST_UNUSED_KEY"
api = "openai_responses"
[providers.fixture.models.test]
id = "fixture-model"
reasoning_effort = "high"
context_window_tokens = 100000
max_output_tokens = 1000
output_reserve_tokens = 1000
"#;

fn launcher(root: &Path) -> Launcher {
    let tools = NativeToolCatalog::open(
        root,
        "PLEXMATON_PICKER_TEST_UNUSED_KEY",
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .expect("permission workspace");
    Launcher {
        permissions: plexmaton_runtime::CodingSessionPermissions::new(&tools),
        root: root.to_owned(),
        workspace: root.to_owned(),
        model: plexmaton_provider::ModelRegistry::parse(CONFIG)
            .expect("model")
            .active_model()
            .clone(),
        ripgrep: "/bin/false".into(),
        driver: "/bin/false".into(),
    }
}
fn agent_id() -> AgentId {
    AgentId::new("primary").expect("agent")
}
fn id(name: &str) -> ConversationId {
    ConversationId::new(name).expect("id")
}
fn key(launcher: &Launcher) -> plexmaton_provider::ApiKey {
    resolve_api_key(&launcher.model, Some(OsString::from("fixture-only"))).expect("key")
}
fn saved(root: &Path, name: &str) -> PathBuf {
    let sessions = ConversationDirectory::under(root).expect("sessions");
    let mut file = sessions
        .create(id(name), UnixMillis::EPOCH)
        .expect("create");
    let mut agent = Agent::for_conversation(
        agent_id(),
        ConversationMetadata::new(id(name), UnixMillis::EPOCH),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    );
    let records = agent.announce("Plexmaton").records.into_iter().chain(
        agent
            .handle_at(
                Input::Submitted {
                    text: "Write a haiku".into(),
                },
                UnixMillis::EPOCH,
            )
            .records,
    );
    for record in records {
        file.append(record).expect("append");
    }
    file.path().to_owned()
}
async fn current(launcher: &Launcher) -> (LiveRuntime, Workspace) {
    let tools = NativeToolCatalog::open(
        &launcher.workspace,
        launcher.model.api_key_env(),
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .expect("tools");
    let mut runtime = LiveRuntime::provider(
        agent_id(),
        "Plexmaton",
        launcher.model.clone(),
        key(launcher),
        tools,
    )
    .expect("runtime");
    runtime
        .use_coding_session(launcher.permissions.clone())
        .expect("coding Session");
    let mut workspace = Workspace::default();
    workspace.emit(std::iter::from_fn(|| runtime.try_next_event()).collect());
    (runtime, workspace)
}

/// SPK-1/JRN-4: previews use real JSONL, bound the result and never repair or create storage.
#[test]
fn listing_is_bounded_read_only_and_rejects_symlinks() {
    let root = FixtureWorkspace::new();
    assert!(
        listing::list(root.path(), &CancellationToken::new())
            .expect("absent")
            .0
            .is_empty()
    );
    assert!(!root.path().join("sessions").exists());
    let path = saved(root.path(), "haiku");
    let before = fs::read(&path).expect("bytes");
    let (entries, limited) = listing::list(root.path(), &CancellationToken::new()).expect("list");
    assert!(!limited);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].title.contains("Write a haiku"));
    assert_eq!(entries[0].id, id("haiku"));
    assert_eq!(fs::read(&path).expect("unchanged"), before);
    std::os::unix::fs::symlink(&path, root.path().join("sessions/link.jsonl")).expect("symlink");
    for n in 0..205 {
        fs::write(root.path().join(format!("sessions/empty-{n}.jsonl")), b"").expect("fixture");
    }
    let (entries, limited) =
        listing::list(root.path(), &CancellationToken::new()).expect("bounded");
    assert!(limited);
    assert_eq!(entries.len(), plexmaton_tui::MAX_CONVERSATION_CHOICES);
    assert!(!entries.iter().any(|entry| entry.id == id("link")));
}

/// SPK-2/JRN-5: a real selected journal installs history without dispatch; failed loads retain the original runtime.
#[tokio::test]
async fn session_switch_validates_before_replacing_and_never_dispatches() {
    let root = FixtureWorkspace::new();
    let path = saved(root.path(), "haiku");
    let launcher = launcher(root.path());
    let (mut runtime, mut workspace) = current(&launcher).await;
    let mut picker = ConversationPicker::new(launcher.clone());
    workspace.begin_conversation_switch();
    let locked = JournalFile::open(&path).expect("lock");
    assert!(
        launcher
            .clone()
            .open_with_key(
                ConversationSelection::Resume(id("haiku")),
                agent_id(),
                JobCancellation::new(),
                key(&launcher)
            )
            .await
            .is_err()
    );
    assert!(
        !picker
            .apply(Update::OpenFailed, &mut runtime, &mut workspace)
            .await
            .expect("refused")
    );
    assert!(picker.current.is_none());
    assert!(!runtime.has_active_work());
    assert!(
        !workspace.conversation_picker_open(),
        "a failed switch leaves nothing waiting; the next one asks again"
    );
    drop(locked);
    workspace.begin_conversation_switch();
    let opened = launcher
        .clone()
        .open_with_key(
            ConversationSelection::Resume(id("haiku")),
            agent_id(),
            JobCancellation::new(),
            key(&launcher),
        )
        .await
        .expect("load");
    assert!(!opened.runtime.has_active_work());
    assert!(
        picker
            .apply(
                Update::Opened(Box::new(opened)),
                &mut runtime,
                &mut workspace
            )
            .await
            .expect("switch")
    );
    assert_eq!(picker.current.as_ref().expect("current").id, id("haiku"));
    assert!(!workspace.conversation_picker_open());
    assert!(
        workspace
            .state()
            .primary_agent()
            .expect("agent")
            .transcript()
            .any(|entry| entry.source == "Write a haiku")
    );
    assert!(!runtime.has_active_work());
    surface_shutdown_report(runtime.shutdown().await.expect("shutdown")).expect("clean");
    drop(runtime);
    fs::write(&path, b"broken middle record\n").expect("corrupt fixture");
    assert!(
        launcher
            .clone()
            .open_with_key(
                ConversationSelection::Resume(id("haiku")),
                agent_id(),
                JobCancellation::new(),
                key(&launcher)
            )
            .await
            .is_err()
    );
}

/// SPK-2/SPK-3: dismissal disposes the loaded candidate and releases its writer; drafts block selection.
#[tokio::test]
async fn cancelled_picker_releases_candidate_and_preserves_current_draft() {
    let root = FixtureWorkspace::new();
    let path = saved(root.path(), "haiku");
    let launcher = launcher(root.path());
    let (mut runtime, mut workspace) = current(&launcher).await;
    let mut picker = ConversationPicker::new(launcher.clone());
    workspace.begin_conversation_switch();
    workspace.return_input(agent_id(), "draft stays here".into());
    picker.select(id("haiku"), &runtime, &mut workspace);
    assert!(picker.job.is_none());
    let opened = launcher
        .clone()
        .open_with_key(
            ConversationSelection::Resume(id("haiku")),
            agent_id(),
            JobCancellation::new(),
            key(&launcher),
        )
        .await
        .expect("load");
    workspace.close_conversation_picker();
    picker.observe_closed(&workspace);
    assert!(picker.cancel.task.is_cancelled());
    assert!(picker.cancel.files.is_cancelled());
    assert!(
        !picker
            .apply(
                Update::Opened(Box::new(opened)),
                &mut runtime,
                &mut workspace
            )
            .await
            .expect("cancel")
    );
    assert_eq!(workspace.state().composer().text(), "draft stays here");
    assert!(picker.current.is_none());
    assert!(
        JournalFile::open(&path).is_ok(),
        "cancel releases writer ownership"
    );
    picker.open(&mut workspace);
    let job = picker.job.as_ref().expect("owned listing").1.id();
    picker.open(&mut workspace);
    picker.select(id("haiku"), &runtime, &mut workspace);
    assert_eq!(
        picker.job.as_ref().expect("same listing").1.id(),
        job,
        "repeated activation never starts another owner"
    );
    picker.shutdown().await.expect("join listing owner");
    assert!(picker.job.is_none());
    surface_shutdown_report(runtime.shutdown().await.expect("shutdown")).expect("clean");
}

/// SPK-2/JRN-4/PER-1: Conversation replacement retains Session authority without creating empty history.
#[tokio::test]
async fn new_session_is_lazy_and_replacement_preserves_saved_history() {
    let root = FixtureWorkspace::new();
    let old_path = saved(root.path(), "haiku");
    let launcher = launcher(root.path());
    let (mut runtime, mut workspace) = current(&launcher).await;
    let expected_permissions = runtime.coding_session().snapshot().expect("Session view");
    let mut picker = ConversationPicker::new(launcher.clone());
    for selection in [
        ConversationSelection::Resume(id("haiku")),
        ConversationSelection::Automatic,
    ] {
        workspace.begin_conversation_switch();
        let opened = launcher
            .clone()
            .open_with_key(
                selection,
                agent_id(),
                JobCancellation::new(),
                key(&launcher),
            )
            .await
            .expect("candidate");
        assert!(
            picker
                .apply(
                    Update::Opened(Box::new(opened)),
                    &mut runtime,
                    &mut workspace
                )
                .await
                .expect("switch")
        );
        assert_eq!(
            runtime
                .coding_session()
                .snapshot()
                .expect("retained view")
                .revision(),
            expected_permissions.revision(),
            "PER-1: /new and /resume retain the coding Session owner"
        );
    }
    let new_session = picker.current.as_ref().expect("planned identity");
    assert_ne!(new_session.id, id("haiku"));
    assert!(!new_session.path.exists());
    assert!(!runtime.has_active_work());
    assert_eq!(
        workspace
            .state()
            .primary_agent()
            .expect("agent")
            .transcript()
            .count(),
        0
    );
    assert!(!workspace.has_unsent_input());
    let old_bytes = fs::read(&old_path).expect("old session survives");
    let reopened = JournalFile::open(&old_path).expect("previous writer released");
    assert_eq!(reopened.journal().conversation_id(), &id("haiku"));
    drop(reopened);
    // A blank replacement can itself be dismissed without ever creating storage.
    workspace.begin_conversation_switch();
    let mut candidate = launcher
        .clone()
        .open_with_key(
            ConversationSelection::Automatic,
            agent_id(),
            JobCancellation::new(),
            key(&launcher),
        )
        .await
        .expect("second candidate");
    let candidate_path = candidate.persisted.take().expect("candidate identity").path;
    workspace.close_conversation_picker();
    picker.observe_closed(&workspace);
    assert!(
        !picker
            .apply(
                Update::Opened(Box::new(candidate)),
                &mut runtime,
                &mut workspace
            )
            .await
            .expect("cancelled")
    );
    assert!(!candidate_path.exists());
    assert_eq!(
        runtime
            .coding_session()
            .snapshot()
            .expect("retained after dismissal")
            .revision(),
        expected_permissions.revision()
    );
    assert_eq!(fs::read(&old_path).expect("retained bytes"), old_bytes);
    surface_shutdown_report(runtime.shutdown().await.expect("shutdown")).expect("clean");
    assert!(!picker.current.as_ref().expect("current").path.exists());
}

/// SPK-2/SPK-3: /new cannot discard a draft or interrupt work; ephemeral construction never persists.
#[tokio::test]
async fn new_session_refuses_unsent_input_and_active_work() {
    let root = FixtureWorkspace::new();
    let launcher = launcher(root.path());
    let (mut runtime, mut workspace) = current(&launcher).await;
    let mut picker = ConversationPicker::new(launcher.clone());
    workspace.return_input(agent_id(), "keep my draft".into());
    picker.new_conversation(&mut workspace, &runtime);
    assert!(picker.job.is_none());
    assert_eq!(workspace.state().composer().text(), "keep my draft");
    workspace.close_conversation_picker();
    let events = std::iter::from_fn(|| runtime.try_next_event()).collect();
    workspace.replace_projection(events);
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "not dispatched to the network".into(),
            },
        )
        .await
        .expect("queue turn");
    assert!(runtime.has_active_work());
    picker.new_conversation(&mut workspace, &runtime);
    assert!(picker.job.is_none());
    assert!(runtime.has_active_work());
    surface_shutdown_report(runtime.shutdown().await.expect("shutdown")).expect("clean");
    let mut opened = launcher
        .clone()
        .open_with_key(
            ConversationSelection::Ephemeral,
            agent_id(),
            JobCancellation::new(),
            key(&launcher),
        )
        .await
        .expect("ephemeral");
    assert!(opened.persisted.is_none());
    surface_shutdown_report(opened.runtime.shutdown().await.expect("shutdown")).expect("clean");
    assert!(!root.path().join("sessions").exists());
}

/// PER-1/PER-7/JRN-4: the real control worker and conversation replacement retain a grant without creating history.
#[tokio::test]
async fn per_7_session_setting_before_first_turn_survives_new_and_revokes_without_jsonl() {
    use crate::permission_controls::PermissionControls;
    use plexmaton_core::NativeFilePreset;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
    };
    let root = FixtureWorkspace::new();
    let launcher = launcher(root.path());
    let mut opened = launcher
        .clone()
        .open_with_key(
            ConversationSelection::Automatic,
            agent_id(),
            JobCancellation::new(),
            key(&launcher),
        )
        .await
        .expect("lazy conversation");
    let old_path = opened
        .persisted
        .as_ref()
        .expect("automatic identity")
        .path
        .clone();
    let mut workspace = Workspace::default();
    workspace.emit(std::iter::from_fn(|| opened.runtime.try_next_event()).collect());
    let mut terminal = Terminal::new(TestBackend::new(95, 24)).expect("terminal");
    let mut controls = PermissionControls::new(opened.runtime.coding_session());
    let keypress = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    controls.open(&mut workspace);
    PermissionControls::publish(controls.next().await, &mut workspace);
    workspace.draw(&mut terminal).expect("settings");
    assert!(
        workspace
            .handle(&keypress(KeyCode::Enter))
            .permission
            .is_none()
    );
    workspace.handle(&keypress(KeyCode::Up));
    let enable = workspace
        .handle(&keypress(KeyCode::Enter))
        .permission
        .expect("reviewed enable");
    controls.apply(enable);
    PermissionControls::publish(controls.next().await, &mut workspace);
    let granted = launcher.permissions.snapshot().expect("enabled");
    assert!(matches!(
        granted.control_view().native_files,
        NativeFilePreset::Enabled(_)
    ));
    assert_eq!(granted.grants().len(), 1);
    assert!(
        !old_path.exists(),
        "setting did not create a Conversation JSONL"
    );
    workspace.handle(&keypress(KeyCode::Esc));

    let mut picker = ConversationPicker::new(launcher.clone());
    picker.current = opened.persisted;
    workspace.begin_conversation_switch();
    let candidate = launcher
        .clone()
        .open_with_key(
            ConversationSelection::Automatic,
            agent_id(),
            JobCancellation::new(),
            key(&launcher),
        )
        .await
        .expect("new candidate");
    let new_path = candidate
        .persisted
        .as_ref()
        .expect("candidate identity")
        .path
        .clone();
    assert!(
        picker
            .apply(
                Update::Opened(Box::new(candidate)),
                &mut opened.runtime,
                &mut workspace
            )
            .await
            .expect("replace conversation")
    );
    assert_eq!(
        opened
            .runtime
            .coding_session()
            .snapshot()
            .expect("retained after new"),
        granted
    );
    assert!(!new_path.exists() && !old_path.exists());

    controls.open(&mut workspace);
    PermissionControls::publish(controls.next().await, &mut workspace);
    workspace.draw(&mut terminal).expect("reopened settings");
    assert!(
        workspace
            .handle(&keypress(KeyCode::Enter))
            .permission
            .is_none()
    );
    workspace.handle(&keypress(KeyCode::Up));
    let revoke = workspace
        .handle(&keypress(KeyCode::Enter))
        .permission
        .expect("reviewed revoke");
    controls.apply(revoke);
    PermissionControls::publish(controls.next().await, &mut workspace);
    assert!(
        opened
            .runtime
            .coding_session()
            .snapshot()
            .expect("revoked")
            .grants()
            .is_empty()
    );
    assert!(!new_path.exists() && !old_path.exists());
    controls.shutdown().await.expect("join controls");
    surface_shutdown_report(opened.runtime.shutdown().await.expect("shutdown")).expect("clean");
}

/// SPK-2/JRN-4: the shell handoff names only its selected saved session; blank plans print nothing.
#[test]
fn continuation_handoff_names_only_the_selected_saved_session() {
    let root = FixtureWorkspace::new();
    let _earlier = saved(root.path(), "earlier");
    let selected = PersistedConversation {
        id: id("latest"),
        path: saved(root.path(), "latest"),
    };
    let mut output = Vec::new();
    report_persisted_conversation(&selected, &mut output).expect("report");
    assert_eq!(
        String::from_utf8(output).expect("text"),
        "To continue this conversation, run:\n  plexmaton resume latest\n"
    );
    let blank = PersistedConversation {
        id: id("blank"),
        path: root.path().join("sessions/blank.jsonl"),
    };
    let mut output = Vec::new();
    report_persisted_conversation(&blank, &mut output).expect("blank");
    assert!(output.is_empty());
}

/// PER-1: a new application owner expires memory; a different physical workspace cannot borrow it.
#[tokio::test]
async fn per_1_coding_session_authority_expires_on_restart_and_refuses_other_workspaces() {
    let root = FixtureWorkspace::new();
    let other = FixtureWorkspace::new();
    let initial = launcher(root.path());
    let restarted = launcher(root.path());
    let foreign = launcher(other.path());
    let (mut runtime, _) = current(&initial).await;
    let before = runtime.coding_session().snapshot().expect("initial view");
    assert_ne!(
        restarted
            .permissions
            .snapshot()
            .expect("restart")
            .revision(),
        before.revision()
    );
    assert!(matches!(
        runtime.use_coding_session(foreign.permissions),
        Err(plexmaton_runtime::RuntimeError::PermissionOwnerMismatch)
    ));
    assert_eq!(
        runtime
            .coding_session()
            .snapshot()
            .expect("unchanged")
            .revision(),
        before.revision()
    );
    surface_shutdown_report(runtime.shutdown().await.expect("shutdown")).expect("clean");
}
