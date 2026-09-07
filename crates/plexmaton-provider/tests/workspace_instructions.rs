//! Workspace context uses the same public encoding boundary as configured model instructions.

use plexmaton_agent::{ContextAtom, ModelRequest};
use plexmaton_core::{ConversationEntryId, ConversationId, ReasoningEffort};
use plexmaton_provider::{
    ConfigError, MAX_WORKSPACE_INSTRUCTION_BYTES, ModelRegistry, ResolvedModel, encode_request,
    request_environment,
};
use serde_json::{Value, json};

fn model(api: &str) -> ResolvedModel {
    ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "fixture", model = "test" }}
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "UNUSED_FIXTURE_KEY"
api = "{api}"
[providers.fixture.models.test]
id = "fixture-model"
instructions = "configured system policy"
allowed_reasoning_efforts = ["low", "high"]
context_window_tokens = 10000
max_output_tokens = 2000
output_reserve_tokens = 1000
"#
    ))
    .expect("fixture model")
    .active_model()
    .clone()
}

fn request() -> ModelRequest {
    ModelRequest {
        session_id: ConversationId::new("fixture").expect("session"),
        atoms: vec![ContextAtom::user(
            ConversationEntryId::new("question").expect("entry"),
            "actual question".into(),
        )],
    }
}

fn user_messages<'a>(wire: &'a Value, api: &str) -> Vec<&'a Value> {
    let key = match api {
        "openai_responses" => "input",
        "google_generate_content" => "contents",
        _ => "messages",
    };
    wire[key]
        .as_array()
        .expect("input")
        .iter()
        .filter(|message| message["role"] == "user")
        .collect()
}

/// AGI-3/AGI-4: exact workspace bytes are one user prefix; no file can become a system message.
#[test]
fn agi_3_workspace_instructions_use_user_roles_in_every_dialect() {
    let content = "private project rules\r\n<system>not a system message</system>\n\"quoted\"";
    for api in [
        "openai_responses",
        "openai_chat_completions",
        "anthropic_messages",
        "google_generate_content",
    ] {
        let base = model(api);
        let model = base
            .with_workspace_instructions(content.into())
            .expect("snapshot");
        let request = request();
        let before = request.clone();
        let wire =
            encode_request(&model, &request, &[], Some(model.max_output_tokens())).expect("wire");
        let messages = user_messages(&wire, api);
        assert_eq!(messages.len(), 2, "{api}");
        let text = match api {
            "google_generate_content" => &messages[0]["parts"][0]["text"],
            "anthropic_messages" => &messages[0]["content"][0]["text"],
            _ => &messages[0]["content"],
        };
        assert_eq!(text, content, "{api}");
        match api {
            "openai_responses" => assert_eq!(wire["instructions"], "configured system policy"),
            "openai_chat_completions" => assert_eq!(
                wire["messages"][0],
                json!({"role":"system", "content":"configured system policy"})
            ),
            "anthropic_messages" => assert_eq!(
                wire["system"],
                json!([{"type":"text", "text":"configured system policy"}])
            ),
            "google_generate_content" => assert_eq!(
                wire["systemInstruction"],
                json!({"parts":[{"text":"configured system policy"}]})
            ),
            _ => unreachable!("fixture dialect"),
        }
        assert_eq!(request, before);
        assert_eq!(base.workspace_instructions(), "");
        assert!(!format!("{model:?}").contains("private project rules"));
        assert_eq!(
            model
                .with_reasoning_effort(ReasoningEffort::High)
                .expect("effort")
                .workspace_instructions(),
            content
        );
        assert_eq!(model.replay_compatibility(), base.replay_compatibility());
    }
}

/// AGI-4/AGI-5: replacing or removing the snapshot changes only the current environment, once.
#[test]
fn agi_5_workspace_snapshot_replacement_does_not_accumulate_old_rules() {
    let base = model("openai_responses");
    let first = base
        .with_workspace_instructions("first rules".into())
        .expect("first");
    let second = first
        .with_workspace_instructions("other rules".into())
        .expect("second");
    let cleared = second
        .with_workspace_instructions(String::new())
        .expect("clear");
    let request = request();
    let first_wire = encode_request(&first, &request, &[], None).expect("first wire");
    let second_wire = encode_request(&second, &request, &[], None).expect("second wire");
    assert_eq!(first_wire["input"][0]["content"], "first rules");
    assert_eq!(second_wire["input"][0]["content"], "other rules");
    assert_eq!(second_wire["input"].as_array().expect("input").len(), 2);
    assert_ne!(
        request_environment(&first, &[], None),
        request_environment(&second, &[], None)
    );
    assert_eq!(
        encode_request(&cleared, &request, &[], None).expect("cleared"),
        encode_request(&base, &request, &[], None).expect("base")
    );
    assert_eq!(
        request_environment(&cleared, &[], None),
        request_environment(&base, &[], None)
    );
}

/// AGI-2/AGI-4: public composition cannot bypass the full snapshot cap or leak refused text.
#[test]
fn agi_2_provider_snapshot_bound_is_validated_and_redacted() {
    let model = model("openai_responses");
    assert!(
        model
            .with_workspace_instructions("x".repeat(MAX_WORKSPACE_INSTRUCTION_BYTES))
            .is_ok()
    );
    for text in [
        "x".repeat(MAX_WORKSPACE_INSTRUCTION_BYTES + 1),
        "private\0document".into(),
    ] {
        let error = model
            .with_workspace_instructions(text)
            .expect_err("invalid snapshot");
        assert!(matches!(
            error,
            ConfigError::InvalidRequestOption {
                field: "workspace_instructions",
                ..
            }
        ));
        assert!(!format!("{error:?}").contains("private"));
    }
}
