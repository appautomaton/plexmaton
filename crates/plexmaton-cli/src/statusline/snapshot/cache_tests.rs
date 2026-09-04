//! Sanitized terminal usage captured from the user-authorized local Luna probe on 2026-09-04.
//! Message/response IDs and output text are fixture scaffolding; numeric usage is retained exactly.
//! All requests in this test terminate at the canned loopback server, never at a configured model.

use super::*;
use crate::session::{SessionSelection, open_selected_session};
use crate::tests::{FixtureWorkspace, fixture_http_server};
use plexmaton_agent::Input;
use plexmaton_core::{AgentId, SessionId};
use plexmaton_runtime::{NativeToolCatalog, RuntimeUpdate};
use std::{ffi::OsString, path::Path, time::Duration};

fn tools(root: &Path, model: &ResolvedModel) -> NativeToolCatalog {
    NativeToolCatalog::open(
        root,
        model.api_key_env(),
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .expect("fixture tools (never executed)")
}

#[tokio::test]
async fn recorded_luna_cache_usage_survives_http_journal_resume_and_shell() {
    // STL-3, TIM-3, JRN-7: prove the whole production path, including actual JSONL reload.
    let root = FixtureWorkspace::new();
    let (url, server) = fixture_http_server([include_str!("fixtures/luna-cache-hit.sse")]);
    let config = super::super::tests::CONFIG.replace("http://127.0.0.1:1/v1", &url);
    let model = plexmaton_provider::ModelRegistry::parse(&config)
        .expect("fixture model")
        .active_model()
        .clone();
    let agent_id = AgentId::new("cache-fixture").expect("agent");
    let session_id = SessionId::new("cache-fixture").expect("session");
    let key = || {
        plexmaton_provider::resolve_api_key(&model, Some(OsString::from("fixture-only")))
            .expect("fixture key")
    };
    let mut opened = open_selected_session(
        root.path(),
        SessionSelection::Create(session_id.clone()),
        agent_id.clone(),
        model.clone(),
        key(),
        tools(root.path(), &model),
    )
    .await
    .expect("create session");
    opened
        .runtime
        .submit(
            agent_id.clone(),
            Input::Submitted {
                text: "fixture input".into(),
            },
        )
        .await
        .expect("submit to mock server");
    tokio::time::timeout(Duration::from_secs(5), async {
        while opened.runtime.has_active_work() {
            match opened.runtime.next_update().await.expect("runtime update") {
                RuntimeUpdate::Report(report) => panic!("unexpected runtime report: {report:?}"),
                RuntimeUpdate::Event(_) | RuntimeUpdate::Finished => {}
            }
        }
    })
    .await
    .expect("fixture did not settle");
    let cwd = root.path().to_str().expect("fixture path");
    let dimensions = Dimensions {
        columns: 95,
        rows: 30,
    };
    let before = serde_json::to_value(
        Snapshot::capture(&opened.runtime, &model, cwd, dimensions).expect("snapshot"),
    )
    .expect("JSON");
    assert_eq!(
        before["plexmaton"]["latest_request"]["terminal"]["usage"]["counts"]["cached_input"],
        3840
    );
    assert_eq!(before["plexmaton"]["usage"]["counts"]["cached_input"], 3840);
    assert_eq!(
        before["context_window"]["current_usage"]["cache_read_input_tokens"],
        3840
    );
    assert_eq!(
        before["context_window"]["current_usage"]["input_tokens"],
        808
    );
    let path = opened
        .persisted
        .as_ref()
        .expect("durable session")
        .path
        .clone();
    opened.runtime.shutdown().await.expect("shutdown");
    drop(opened);
    assert_eq!(
        server
            .join()
            .expect("fixture server thread")
            .expect("fixture HTTP request")
            .len(),
        1
    );
    let disk = std::fs::read_to_string(path).expect("JSONL");
    let terminal: serde_json::Value = disk
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|record| record["kind"] == "request_attempt_finished")
        .expect("terminal JSONL record");
    assert_eq!(
        terminal["fact"]["terminal"]["usage"]["counts"]["cached_input"],
        3840
    );

    let mut reopened = open_selected_session(
        root.path(),
        SessionSelection::Resume(session_id),
        agent_id,
        model.clone(),
        key(),
        tools(root.path(), &model),
    )
    .await
    .expect("resume without another HTTP request");
    let after = serde_json::to_value(
        Snapshot::capture(&reopened.runtime, &model, cwd, dimensions).expect("restored snapshot"),
    )
    .expect("JSON");
    reopened
        .runtime
        .shutdown()
        .await
        .expect("shutdown restored session");
    assert_eq!(before, after);

    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/statusline-pastel.sh");
    let config = super::super::config::StatusLineConfig {
        command: format!("bash '{}'", script.display()),
        max_rows: 6,
        timeout_ms: 5000,
        refresh_ms: None,
    };
    let text = super::super::process::execute(
        &config,
        serde_json::to_vec(&after).expect("snapshot stdin"),
        root.path(),
        model.api_key_env(),
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("status script");
    let text = text
        .lines()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("cache 83%"), "{text}");
    assert!(text.contains(" 4.6k/272.0k 1%"), "{text}");
}
