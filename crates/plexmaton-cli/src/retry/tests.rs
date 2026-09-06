use super::*;
use crate::tests::{FixtureWorkspace, fixture_http_responses};
use plexmaton_core::SessionId;
use plexmaton_runtime::RuntimeUpdate;

const CONFIG: &str = r#"
active_model = { provider = "fixture", model = "test" }
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "PLEXMATON_TEST_UNUSED_KEY"
api = "openai_responses"
[providers.fixture.models.test]
id = "fixture-model"
reasoning_effort = "high"
context_window_tokens = 100000
max_output_tokens = 1000
output_reserve_tokens = 1000
"#;

async fn settle(runtime: &mut LiveRuntime) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while runtime.has_active_work() {
            match runtime.next_update().await.expect("fixture update") {
                RuntimeUpdate::Report(report) => panic!("unexpected report: {report:?}"),
                RuntimeUpdate::Event(_) | RuntimeUpdate::Finished => {}
            }
        }
        while runtime.try_next_event().is_some() {}
    })
    .await
    .expect("fixture settles");
}

/// JRN-3/JRN-5, PRV-4: actual HTTP encoding and JSONL reopen preserve retry input byte-for-byte;
/// ordinary continuation emits consecutive user messages in both supported codecs.
#[tokio::test]
async fn retry_and_normal_continuation_survive_jsonl_and_both_wire_codecs() {
    for api in ["openai_responses", "openai_chat_completions"] {
        let root = FixtureWorkspace::new();
        let (url, server) = fixture_http_responses([(429, "{}"), (429, "{}"), (429, "{}")]);
        let config = CONFIG
            .replace("http://127.0.0.1:1/v1", &url)
            .replace("openai_responses", api);
        let model = plexmaton_provider::ModelRegistry::parse(&config)
            .expect("model")
            .active_model()
            .clone();
        let agent = AgentId::new("fixture-agent").expect("agent");
        let id = SessionId::new("retry-fixture").expect("session");
        let open = |selection| {
            let key = resolve_api_key(&model, Some(OsString::from("fixture-only"))).expect("key");
            let tools = NativeToolCatalog::open(
                root.path(),
                model.api_key_env(),
                "/bin/false",
                "/bin/false",
                Vec::new(),
            )
            .expect("tools never run");
            open_selected_session(
                root.path(),
                selection,
                agent.clone(),
                model.clone(),
                key,
                tools,
            )
        };
        let mut session = open(SessionSelection::Create(id.clone()))
            .await
            .expect("create");
        session
            .runtime
            .submit(
                agent.clone(),
                Input::Submitted {
                    text: "original".into(),
                },
            )
            .await
            .expect("submit");
        settle(&mut session.runtime).await;
        let target = session
            .runtime
            .retry_candidate()
            .expect("HTTP 429 retry")
            .target;
        session.runtime.shutdown().await.expect("close");
        drop(session);
        let mut session = open(SessionSelection::Resume(id)).await.expect("resume");
        assert_eq!(
            session
                .runtime
                .retry_candidate()
                .expect("restored action")
                .target,
            target
        );
        session.runtime.retry(target, None).await.expect("retry");
        settle(&mut session.runtime).await;
        session
            .runtime
            .submit(
                agent,
                Input::Submitted {
                    text: "next question".into(),
                },
            )
            .await
            .expect("continue");
        settle(&mut session.runtime).await;
        session.runtime.shutdown().await.expect("close");
        let bodies = server
            .join()
            .expect("fixture thread")
            .expect("request bodies");
        assert_eq!(
            bodies[0], bodies[1],
            "retry changed encoded context for {api}"
        );
        let body: serde_json::Value = serde_json::from_slice(&bodies[2]).expect("JSON");
        let messages = body[if api == "openai_responses" {
            "input"
        } else {
            "messages"
        }]
        .as_array()
        .expect("messages");
        let users = messages
            .iter()
            .filter(|item| item["role"] == "user")
            .collect::<Vec<_>>();
        assert_eq!(users.len(), 2, "{body}");
        assert!(users[0].to_string().contains("original"));
        assert!(users[1].to_string().contains("next question"));
    }
}
