use super::*;
use crate::ModelChangeRefusal as Refusal;
use plexmaton_core::ReasoningEffort;

/// EFF-1/PRV-6: an idle replacement updates real wire encoding/environment with no journal event.
#[tokio::test]
async fn effort_replacement_updates_wire_and_environment_without_rewriting_history() {
    let directory = tools::TestWorkspace::new("effort");
    let config = r#"
active_model = { provider = "fixture", model = "test" }
[providers.fixture]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "TEST_KEY"
api = "openai_responses"
[providers.fixture.models.test]
id = "fixture"
reasoning_effort = "high"
allowed_reasoning_efforts = ["low", "high", "max"]
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 1024
"#;
    let model = plexmaton_provider::ModelRegistry::parse(config)
        .expect("config")
        .active_model()
        .clone();
    let key =
        plexmaton_provider::resolve_api_key(&model, Some("fixture-only".into())).expect("key");
    let mut runtime = LiveRuntime::provider(agent_id(), "Fixture", model, key, directory.catalog())
        .expect("runtime");
    while runtime.try_next_event().is_some() {}
    let records = runtime.agent.journal().records().len();
    let previous = runtime.driver.request_environment().clone();
    runtime
        .set_reasoning_effort(&agent_id(), ReasoningEffort::Max)
        .expect("idle change");
    assert_ne!(runtime.driver.request_environment(), &previous);
    let (model, tools) = runtime.driver.budget_inputs().expect("model inputs");
    let body = plexmaton_provider::encode_request(
        model,
        &plexmaton_agent::ModelRequest {
            session_id: runtime.agent.journal().conversation_id().clone(),
            atoms: Vec::new(),
        },
        tools,
        Some(model.max_output_tokens()),
    )
    .expect("wire request");
    assert_eq!(body["reasoning"]["effort"], "max");
    let selected = model.clone();
    assert_eq!(runtime.agent.journal().records().len(), records);
    assert!(runtime.try_next_event().is_none());
    assert_eq!(
        runtime.set_reasoning_effort(&agent_id(), ReasoningEffort::Medium),
        Err(Refusal::Unsupported)
    );
    assert_eq!(runtime.configured_model(), Some(&selected));
    runtime.shutdown().await.expect("shutdown");
    assert_eq!(
        runtime.set_reasoning_effort(&agent_id(), ReasoningEffort::Low),
        Err(Refusal::ShuttingDown)
    );
}

/// EFF-1: a turn and its queued successor retain their driver; failure cannot change the target.
#[tokio::test]
async fn effort_changes_refuse_active_and_queued_work() {
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(driver);
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "first".to_owned(),
            },
        )
        .await
        .expect("submit");
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "queued".to_owned(),
            },
        )
        .await
        .expect("queue");
    assert_eq!(
        runtime.set_reasoning_effort(&agent_id(), ReasoningEffort::Max),
        Err(Refusal::Busy)
    );
    assert_eq!(
        runtime.set_reasoning_effort(&AgentId::new("other").expect("id"), ReasoningEffort::Max),
        Err(Refusal::WrongAgent)
    );
    runtime.shutdown().await.expect("shutdown");
}

/// MDL-1/MDL-3: an idle cross-provider replacement changes encoding/environment and limits,
/// retains workspace guidance, and refuses credentials not excluded from native commands.
#[tokio::test]
async fn model_replacement_is_atomic_and_preserves_workspace_instructions() {
    let directory = tools::TestWorkspace::new("model-change");
    let registry = model_registry();
    let original = registry
        .active_model()
        .with_workspace_instructions("Workspace guidance".into())
        .expect("guidance");
    let destination = registry.model("second", "same").expect("second").clone();
    let key = |model: &plexmaton_provider::ResolvedModel| {
        plexmaton_provider::resolve_api_key(model, Some("fixture-only".into())).expect("key")
    };
    let mut unprotected = LiveRuntime::provider(
        agent_id(),
        "Fixture",
        original.clone(),
        key(&original),
        directory.catalog(),
    )
    .expect("runtime");
    assert_eq!(
        unprotected.set_model(&agent_id(), destination.clone(), key(&destination)),
        Err(Refusal::UnprotectedCredential)
    );
    assert_eq!(unprotected.configured_model(), Some(&original));
    unprotected.shutdown().await.expect("shutdown");
    let catalog = directory
        .catalog()
        .with_provider_credentials(registry.models().map(|model| model.api_key_env()));
    let mut runtime = LiveRuntime::provider(
        agent_id(),
        "Fixture",
        original.clone(),
        key(&original),
        catalog,
    )
    .expect("runtime");
    while runtime.try_next_event().is_some() {}
    let records = runtime.agent.journal().records().len();
    let environment = runtime.driver.request_environment().clone();
    let selected = runtime
        .set_model(&agent_id(), destination.clone(), key(&destination))
        .expect("switch");
    assert_eq!(selected.workspace_instructions(), "Workspace guidance");
    assert_eq!(selected.instructions(), "Second model instructions");
    assert_eq!(selected.reasoning_effort(), ReasoningEffort::High);
    assert_eq!(selected.context_window_tokens(), 4096);
    assert_ne!(runtime.driver.request_environment(), &environment);
    assert_eq!(runtime.agent.journal().records().len(), records);
    assert!(runtime.try_next_event().is_none());
    let (model, tools) = runtime.driver.budget_inputs().expect("driver");
    let body = plexmaton_provider::encode_request(
        model,
        &plexmaton_agent::ModelRequest {
            session_id: runtime.agent.journal().conversation_id().clone(),
            atoms: Vec::new(),
        },
        tools,
        Some(model.max_output_tokens()),
    )
    .expect("wire");
    assert_eq!(body["model"], "second-wire");
    assert_eq!(body["max_tokens"], 512);
    assert_eq!(
        runtime.set_model(
            &AgentId::new("wrong").expect("agent"),
            original.clone(),
            key(&original)
        ),
        Err(Refusal::WrongAgent)
    );
    assert_eq!(runtime.configured_model(), Some(&selected));
    runtime.shutdown().await.expect("shutdown");
    assert_eq!(
        runtime.set_model(&agent_id(), original.clone(), key(&original)),
        Err(Refusal::ShuttingDown)
    );
}

fn model_registry() -> plexmaton_provider::ModelRegistry {
    plexmaton_provider::ModelRegistry::parse(
        r#"
active_model = { provider = "first", model = "same" }
[providers.first]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "TEST_KEY"
api = "openai_responses"
[providers.first.models.same]
id = "first-wire"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 1024
[providers.second]
base_url = "http://127.0.0.1:9"
api_key_env = "OTHER_LOGIN"
api = "anthropic_messages"
[providers.second.models.same]
id = "second-wire"
instructions = "Second model instructions"
reasoning_effort = "high"
allowed_reasoning_efforts = ["low", "high"]
context_window_tokens = 4096
max_output_tokens = 512
output_reserve_tokens = 512
"#,
    )
    .expect("registry")
}

/// MDL-1/PRV-3: replay rejection leaves the driver and canonical journal unchanged; busy refuses first.
#[tokio::test]
async fn model_replacement_refuses_busy_and_incompatible_replay_without_mutation() {
    let directory = tools::TestWorkspace::new("model-replay");
    let registry = model_registry();
    let original = registry.active_model().clone();
    let destination = registry.model("second", "same").expect("model").clone();
    let key = |model: &plexmaton_provider::ResolvedModel| {
        plexmaton_provider::resolve_api_key(model, Some("fixture-only".into())).expect("key")
    };
    let mut runtime = LiveRuntime::provider(
        agent_id(),
        "Fixture",
        original.clone(),
        key(&original),
        directory
            .catalog()
            .with_provider_credentials(registry.models().map(|model| model.api_key_env())),
    )
    .expect("runtime");
    // Construct acknowledged history through the semantic owner, without a network task.
    runtime.agent.handle_at(
        Input::Submitted {
            text: "Keep this history".into(),
        },
        UnixMillis::EPOCH,
    );
    assert_eq!(
        runtime.set_model(&agent_id(), destination.clone(), key(&destination)),
        Err(Refusal::Busy)
    );
    let step_id = runtime.agent.active_model_step().expect("step");
    let mut codec = plexmaton_provider::ProviderCodec::new(
        &RequestAttemptId::new("model-replay").expect("attempt"),
        &original,
        plexmaton_provider::DecodeLimits::production(),
    );
    for (kind, payload) in [
        (
            "response.output_item.done",
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs_fixture","summary":[],"encrypted_content":"private-fixture"}}"#,
        ),
        (
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"completed","usage":null}}"#,
        ),
    ] {
        for event in codec.push_sse(kind, payload).expect("decode") {
            runtime.agent.handle_at(
                Input::Streamed {
                    step_id: step_id.clone(),
                    event,
                },
                UnixMillis::EPOCH,
            );
        }
    }
    assert!(!runtime.agent.is_running());
    let before = serde_json::to_value(runtime.agent.journal().records()).expect("records");
    assert_eq!(
        runtime.set_model(&agent_id(), destination.clone(), key(&destination)),
        Err(Refusal::IncompatibleHistory)
    );
    assert_eq!(runtime.configured_model(), Some(&original));
    assert_eq!(
        serde_json::to_value(runtime.agent.journal().records()).expect("records"),
        before
    );
    runtime.shutdown().await.expect("shutdown");
}
