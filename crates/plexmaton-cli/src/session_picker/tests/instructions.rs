use super::*;
use crate::tests::fixture_http_server;
use std::time::Duration;

fn instruction_launcher(root: &Path, url: &str) -> Launcher {
    let workspace = root.join("workspace");
    fs::create_dir(&workspace).expect("workspace");
    let home = root.join("home");
    fs::create_dir(&home).expect("home");
    let mut launch = launcher(&workspace);
    launch.root = home;
    launch.model = plexmaton_provider::ModelRegistry::parse(
        &CONFIG
            .replace("http://127.0.0.1:9/v1", url)
            .replace("openai_responses", "openai_chat_completions"),
    )
    .expect("loopback model")
    .active_model()
    .clone();
    launch
}

async fn submit_and_settle(runtime: &mut LiveRuntime, text: &str) {
    runtime
        .submit(agent_id(), Input::Submitted { text: text.into() })
        .await
        .expect("submit");
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.has_active_work() {
            let _event = runtime.next_event().await.expect("runtime event");
        }
    })
    .await
    .expect("fixture completes");
    while runtime.try_next_event().is_some() {}
}

/// AGI-3/AGI-5/JRN-4: real requests keep one snapshot until reopen; loading alone writes no history.
#[tokio::test]
async fn agi_5_wire_snapshot_is_stable_then_refreshes_on_jsonl_resume() {
    let root = FixtureWorkspace::new();
    const ANSWER: &str =
        include_str!("../../../../plexmaton-provider/tests/fixtures/chat_final_answer.sse");
    let (url, server) = fixture_http_server([ANSWER, ANSWER, ANSWER]);
    let launch = instruction_launcher(root.path(), &url);
    let source = launch.workspace.join("AGENTS.md");
    fs::write(&source, "initial project rule").expect("initial rule");
    let mut opened = launch
        .clone()
        .open_with_key(
            ConversationSelection::Automatic,
            agent_id(),
            JobCancellation::new(),
            key(&launch),
        )
        .await
        .expect("open");
    let persisted = opened.persisted.as_ref().expect("automatic identity");
    let path = persisted.path.clone();
    let session_id = persisted.id.clone();
    assert!(!path.exists());
    assert!(!launch.root.join("sessions").exists());
    assert!(!opened.runtime.has_active_work());
    submit_and_settle(&mut opened.runtime, "first question").await;
    fs::write(&source, "updated project rule").expect("edit instructions");
    submit_and_settle(&mut opened.runtime, "second question").await;
    let snapshot = opened
        .runtime
        .configured_model()
        .expect("model")
        .workspace_instructions()
        .to_owned();
    assert!(snapshot.contains("initial project rule"));
    assert!(!snapshot.contains("updated project rule"));
    let before = opened
        .runtime
        .acknowledged_conversation()
        .expect("journal")
        .0
        .clone();
    opened.runtime.shutdown().await.expect("shutdown");
    drop(opened);
    let bytes = fs::read(&path).expect("saved journal");
    assert!(!String::from_utf8_lossy(&bytes).contains("initial project rule"));
    let mut resumed = launch
        .clone()
        .open_with_key(
            ConversationSelection::Resume(session_id),
            agent_id(),
            JobCancellation::new(),
            key(&launch),
        )
        .await
        .expect("resume");
    assert_eq!(fs::read(&path).expect("unchanged bytes"), bytes);
    assert_eq!(
        resumed
            .runtime
            .acknowledged_conversation()
            .expect("replayed")
            .0,
        &before
    );
    assert!(!resumed.runtime.has_active_work());
    let current = resumed
        .runtime
        .configured_model()
        .expect("current model")
        .workspace_instructions();
    assert!(current.contains("updated project rule"));
    assert!(!current.contains("initial project rule"));
    submit_and_settle(&mut resumed.runtime, "third question").await;
    resumed.runtime.shutdown().await.expect("shutdown resumed");
    let requests = server
        .join()
        .expect("join fixture")
        .expect("exactly three requests");
    assert_eq!(requests.len(), 3);
    for (index, bytes) in requests.iter().enumerate() {
        let body: serde_json::Value = serde_json::from_slice(bytes).expect("request JSON");
        let messages = body["messages"].as_array().expect("messages");
        assert_eq!(messages[0]["role"], "user");
        let context = messages[0]["content"].as_str().expect("instruction prefix");
        let expected = if index == 2 {
            "updated project rule"
        } else {
            "initial project rule"
        };
        assert!(context.contains(expected));
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["content"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("Plexmaton workspace instructions")))
                .count(),
            1
        );
        assert_eq!(messages[1]["content"], "first question");
        assert_eq!(
            messages.last().expect("question")["content"],
            ["first question", "second question", "third question"][index]
        );
    }
}

/// AGI-2/AGI-5: invalid or cancelled replacement loads cannot replace the live snapshot or create storage.
#[tokio::test]
async fn agi_5_failed_or_cancelled_instruction_load_preserves_the_open_runtime() {
    let root = FixtureWorkspace::new();
    let launch = instruction_launcher(root.path(), "http://127.0.0.1:9/v1");
    let source = launch.workspace.join("AGENTS.md");
    fs::write(&source, "keep this snapshot").expect("initial rule");
    let mut opened = launch
        .clone()
        .open_with_key(
            ConversationSelection::Automatic,
            agent_id(),
            JobCancellation::new(),
            key(&launch),
        )
        .await
        .expect("open");
    let original = opened.runtime.configured_model().expect("model").clone();
    fs::write(&source, [0xff]).expect("invalid replacement");
    assert!(
        launch
            .clone()
            .open_with_key(
                ConversationSelection::Automatic,
                agent_id(),
                JobCancellation::new(),
                key(&launch)
            )
            .await
            .is_err()
    );
    let cancel = JobCancellation::new();
    cancel.cancel();
    assert!(
        launch
            .clone()
            .open_with_key(
                ConversationSelection::Automatic,
                agent_id(),
                cancel,
                key(&launch)
            )
            .await
            .is_err()
    );
    assert_eq!(opened.runtime.configured_model(), Some(&original));
    assert!(!opened.runtime.has_active_work());
    assert!(!launch.root.join("sessions").exists());
    opened.runtime.shutdown().await.expect("shutdown");
}
