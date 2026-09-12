//! Offline witness for reading Chat history under a Responses default, without replaying work.
use super::*;
use crate::session::{ConversationSelection, open_selected_conversation};
use crate::tests::{FixtureWorkspace, fixture_http_server};
use plexmaton_agent::Input;
use plexmaton_core::{AgentId, ConversationId};
use plexmaton_provider::{ContextBudgetError, EncodeError, ModelRegistry};
use plexmaton_runtime::{
    ContextBudgetSnapshot, ModelChangeRefusal, NativeToolCatalog, RuntimeUpdate,
};
use std::{path::Path, time::Duration};

fn tools(root: &Path, model: &ResolvedModel) -> NativeToolCatalog {
    NativeToolCatalog::open(
        root,
        model.api_key_env(),
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .expect("fixture tools, never executed")
}

/// STL-3/MDL-1/MDL-4: reading history and observing facts do not require provider compatibility.
#[tokio::test]
async fn status_resume_isolates_context_refusal_without_weakening_model_admission() {
    let root = FixtureWorkspace::new();
    let (url, server) = fixture_http_server([concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"private-reasoning-marker\\n\\n\",\"content\":\"fixture answer\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":20,\"total_tokens\":120}}\n\n",
        "data: [DONE]\n\n"
    )]);
    let config = super::super::tests::CONFIG.replace("http://127.0.0.1:1/v1", &url);
    let responses = ModelRegistry::parse(&config)
        .expect("Responses")
        .active_model()
        .clone();
    let chat = ModelRegistry::parse(&config.replace("openai_responses", "openai_chat_completions"))
        .expect("Chat")
        .active_model()
        .clone();
    let key = |model: &ResolvedModel| {
        plexmaton_provider::resolve_api_key(model, Some("fixture-only".into())).expect("key")
    };
    let agent = AgentId::new("fixture").expect("agent");
    let session = ConversationId::new("cross-dialect").expect("session");
    let mut opened = open_selected_conversation(
        root.path(),
        ConversationSelection::Create(session.clone()),
        agent.clone(),
        chat.clone(),
        key(&chat),
        tools(root.path(), &chat),
    )
    .await
    .expect("create fixture conversation");
    opened
        .runtime
        .submit(
            agent.clone(),
            Input::Submitted {
                text: "private-prompt-marker".into(),
            },
        )
        .await
        .expect("submit to loopback");
    tokio::time::timeout(Duration::from_secs(5), async {
        while opened.runtime.has_active_work() {
            match opened.runtime.next_update().await.expect("runtime update") {
                RuntimeUpdate::Event(_) | RuntimeUpdate::Finished => {}
                RuntimeUpdate::Report(report) => panic!("unexpected report: {report:?}"),
            }
        }
    })
    .await
    .expect("fixture settles");
    assert_eq!(
        opened
            .runtime
            .set_model(&agent, responses.clone(), key(&responses)),
        Err(ModelChangeRefusal::IncompatibleHistory)
    );
    let dimensions = Dimensions {
        columns: 95,
        rows: 30,
    };
    let before = serde_json::to_value(Snapshot::capture(
        &opened.runtime,
        &chat,
        "/fixture/project",
        dimensions,
    ))
    .expect("JSON");
    let path = opened.persisted.as_ref().expect("journal").path.clone();
    opened.runtime.shutdown().await.expect("shutdown");
    drop(opened);
    assert_eq!(
        server
            .join()
            .expect("server thread")
            .expect("fixture requests")
            .len(),
        1
    );
    let disk = std::fs::read(&path).expect("journal bytes");
    let mut reopened = open_selected_conversation(
        root.path(),
        ConversationSelection::Resume(session),
        agent.clone(),
        responses.clone(),
        key(&responses),
        tools(root.path(), &responses),
    )
    .await
    .expect("history can be opened under the configured default");
    assert!(matches!(
        reopened.runtime.context_budget(),
        Err(ContextBudgetError::Encoding(
            EncodeError::IncompatibleReplay { .. }
        ))
    ));
    let after = serde_json::to_value(Snapshot::capture(
        &reopened.runtime,
        &responses,
        "/fixture/project",
        dimensions,
    ))
    .expect("JSON");
    assert_eq!(after["plexmaton"]["context"]["availability"], "unavailable");
    assert_eq!(
        after["plexmaton"]["context"]["reason"],
        "history_incompatible"
    );
    assert!(after["context_window"]["used_percentage"].is_null());
    for field in ["session_id", "cost"] {
        assert_eq!(after[field], before[field]);
    }
    for field in [
        "usage",
        "latest_request",
        "turn",
        "head",
        "created_at_unix_ms",
    ] {
        assert_eq!(
            after["plexmaton"][field], before["plexmaton"][field],
            "{field}"
        );
    }
    assert_eq!(after["context_window"]["total_input_tokens"], 100);
    assert_eq!(after["context_window"]["total_output_tokens"], 20);
    let encoded = serde_json::to_string(&after).expect("JSON");
    for secret in [
        "private-prompt-marker",
        "private-reasoning-marker",
        "fixture-only",
        "127.0.0.1",
    ] {
        assert!(!encoded.contains(secret));
    }
    status_command_keeps_partial_snapshot(&reopened.runtime, &responses).await;
    reopened
        .runtime
        .set_model(&agent, chat.clone(), key(&chat))
        .expect("return to compatible model");
    assert!(matches!(
        reopened.runtime.context_budget(),
        Ok(ContextBudgetSnapshot::Available(_))
    ));
    reopened
        .runtime
        .shutdown()
        .await
        .expect("shutdown restored session");
    assert_eq!(std::fs::read(path).expect("journal after inspection"), disk);
}

async fn status_command_keeps_partial_snapshot(runtime: &LiveRuntime, model: &ResolvedModel) {
    use super::super::{StatusLine, StatusLineConfig, Update};
    use plexmaton_tui::{Palette, Workspace};
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/statusline-pastel.sh");
    let mut status = StatusLine::new(
        StatusLineConfig {
            command: format!("bash '{}'", script.display()),
            max_rows: 6,
            timeout_ms: 5000,
            refresh_ms: None,
        },
        model.clone(),
        "/".into(),
    );
    let mut workspace = Workspace::with_palette(Palette::pastel());
    for columns in [120, 95, 60] {
        status.capture(runtime, Dimensions { columns, rows: 30 }, &mut workspace);
        let update = tokio::time::timeout(Duration::from_secs(6), status.next())
            .await
            .expect("bounded status command");
        let Update::Output(Ok(text)) = update else {
            panic!("partial snapshot must reach the script: {update:?}")
        };
        let output = text
            .lines()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        for retained in [
            "Luna",
            "↑100",
            "↓20",
            "History incompatible with model · /model",
        ] {
            assert!(output.contains(retained), "{columns}: {output}");
        }
        assert!(
            !output.contains(""),
            "incompatible current capacity must not label historical input"
        );
        status.apply(Ok(text), &mut workspace);
    }
    status.shutdown().await.expect("status shutdown");
}
