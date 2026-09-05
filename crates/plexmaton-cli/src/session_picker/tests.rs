use super::*;
use crate::tests::FixtureWorkspace;
use plexmaton_agent::{Agent, ApprovalPolicy, SessionMetadata, TurnBudget, UnixMillis};
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
    Launcher {
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
fn id(name: &str) -> SessionId {
    SessionId::new(name).expect("id")
}
fn key(launcher: &Launcher) -> plexmaton_provider::ApiKey {
    resolve_api_key(&launcher.model, Some(OsString::from("fixture-only"))).expect("key")
}
fn saved(root: &Path, name: &str) -> PathBuf {
    let sessions = SessionDirectory::under(root).expect("sessions");
    let mut file = sessions
        .create(id(name), UnixMillis::EPOCH)
        .expect("create");
    let mut agent = Agent::for_session(
        agent_id(),
        SessionMetadata::new(id(name), UnixMillis::EPOCH),
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
    let mut runtime = LiveRuntime::openai(
        agent_id(),
        "Plexmaton",
        launcher.model.clone(),
        key(launcher),
        tools,
    )
    .expect("runtime");
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
    assert_eq!(entries.len(), plexmaton_tui::MAX_SESSION_CHOICES);
    assert!(!entries.iter().any(|entry| entry.id == id("link")));
}

/// SPK-2/JRN-5: a real selected journal installs history without dispatch; failed loads retain the original runtime.
#[tokio::test]
async fn session_switch_validates_before_replacing_and_never_dispatches() {
    let root = FixtureWorkspace::new();
    let path = saved(root.path(), "haiku");
    let launcher = launcher(root.path());
    let (mut runtime, mut workspace) = current(&launcher).await;
    let mut picker = SessionPicker::new(launcher.clone());
    workspace.open_session_picker();
    let locked = JournalFile::open(&path).expect("lock");
    assert!(
        launcher
            .clone()
            .resume_with_key(
                id("haiku"),
                agent_id(),
                CancellationToken::new(),
                key(&launcher)
            )
            .await
            .is_err()
    );
    assert!(
        !picker
            .apply(
                Update::Failed(SessionPickerStatus::OpenFailed),
                &mut runtime,
                &mut workspace
            )
            .await
            .expect("refused")
    );
    assert!(picker.current.is_none());
    assert!(!runtime.has_active_work());
    drop(locked);
    let opened = launcher
        .clone()
        .resume_with_key(
            id("haiku"),
            agent_id(),
            CancellationToken::new(),
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
    assert!(!workspace.session_picker_open());
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
            .resume_with_key(
                id("haiku"),
                agent_id(),
                CancellationToken::new(),
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
    let mut picker = SessionPicker::new(launcher.clone());
    workspace.open_session_picker();
    workspace.return_input(agent_id(), "draft stays here".into());
    picker.select(id("haiku"), &runtime, &mut workspace);
    assert!(picker.job.is_none());
    let opened = launcher
        .clone()
        .resume_with_key(
            id("haiku"),
            agent_id(),
            CancellationToken::new(),
            key(&launcher),
        )
        .await
        .expect("load");
    workspace.close_session_picker();
    picker.observe_closed(&workspace);
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
    let job = picker.job.as_ref().expect("owned listing").id();
    picker.open(&mut workspace);
    picker.select(id("haiku"), &runtime, &mut workspace);
    assert_eq!(
        picker.job.as_ref().expect("same listing").id(),
        job,
        "repeated activation never starts another owner"
    );
    picker.shutdown().await.expect("join listing owner");
    assert!(picker.job.is_none());
    surface_shutdown_report(runtime.shutdown().await.expect("shutdown")).expect("clean");
}
