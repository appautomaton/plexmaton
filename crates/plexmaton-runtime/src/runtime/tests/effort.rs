use super::*;
use crate::EffortChangeRefusal as Refusal;
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
