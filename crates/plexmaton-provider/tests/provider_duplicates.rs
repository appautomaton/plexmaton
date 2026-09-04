use std::convert::Infallible;

use futures_util::stream;
use plexmaton_agent::ModelEvent;
use plexmaton_provider::{
    DecodeLimits, ModelApi, ModelRegistry, OpenAiCodec, ResolvedModel, SseDecodeError, drive_sse,
};

const CHAT_DUPLICATE_TOOL_CALL_ID: &str = include_str!("fixtures/chat_duplicate_tool_call_id.sse");
const RESPONSES_DUPLICATE_TOOL_CALL_ID: &str =
    include_str!("fixtures/responses_duplicate_tool_call_id.sse");

fn profile(api: ModelApi) -> ResolvedModel {
    let api = match api {
        ModelApi::OpenaiResponses => "openai_responses",
        ModelApi::OpenaiChatCompletions => "openai_chat_completions",
    };
    ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "test", model = "fixture" }}
[providers.test]
base_url = "http://127.0.0.1:8317/v1"
api_key_env = "TEST_KEY"
[providers.test.models.fixture]
api = "{api}"
id = "fixture"
reasoning_effort = "low"
context_window_tokens = 100000
max_output_tokens = 10000
output_reserve_tokens = 5000
"#
    ))
    .unwrap_or_else(|error| panic!("fixture profile: {error}"))
    .active_model()
    .clone()
}

#[tokio::test]
async fn prv_2_rejects_duplicate_chat_tool_call_ids_before_emission() {
    let profile = profile(ModelApi::OpenaiChatCompletions);
    let source = stream::iter([Ok::<_, Infallible>(CHAT_DUPLICATE_TOOL_CALL_ID.as_bytes())]);
    let mut emitted = Vec::new();

    let result = drive_sse(&profile, source, DecodeLimits::production(), |event| {
        emitted.push(event);
        std::future::ready(())
    })
    .await;
    let Err(error) = result else {
        panic!("duplicate Chat call unexpectedly decoded");
    };

    assert!(matches!(
        error,
        SseDecodeError::Decode(plexmaton_provider::DecodeError::DuplicateToolCallId {
            call_id
        }) if call_id.as_str() == "call_duplicate"
    ));
    assert!(
        emitted.is_empty(),
        "a rejected Chat batch must emit no call or stop"
    );
}

#[tokio::test]
async fn prv_2_rejects_incremental_duplicate_responses_tool_call_ids() {
    let profile = profile(ModelApi::OpenaiResponses);
    let source = stream::iter([Ok::<_, Infallible>(
        RESPONSES_DUPLICATE_TOOL_CALL_ID.as_bytes(),
    )]);
    let mut emitted = Vec::new();

    let result = drive_sse(&profile, source, DecodeLimits::production(), |event| {
        emitted.push(event);
        std::future::ready(())
    })
    .await;
    let Err(error) = result else {
        panic!("duplicate Responses call unexpectedly decoded");
    };

    assert!(matches!(
        error,
        SseDecodeError::Decode(plexmaton_provider::DecodeError::DuplicateToolCallId {
            call_id
        }) if call_id.as_str() == "call_duplicate"
    ));
    assert!(
        matches!(emitted.as_slice(), [ModelEvent::Called { call, .. }]
        if call.call_id.as_str() == "call_duplicate" && call.arguments.contains("one"))
    );
}

#[test]
fn prv_2_responses_counts_incrementally_completed_calls_toward_the_step_bound() {
    let profile = profile(ModelApi::OpenaiResponses);
    let mut limits = DecodeLimits::production();
    limits.max_tool_calls = 1;
    let mut codec = OpenAiCodec::new(&profile, limits);
    let first = r#"{"type":"response.output_item.done","output_index":0,"item":{"id":"fc_one","type":"function_call","call_id":"call_one","name":"read_file","arguments":"{}","status":"completed"}}"#;
    let second = r#"{"type":"response.output_item.done","output_index":1,"item":{"id":"fc_two","type":"function_call","call_id":"call_two","name":"read_file","arguments":"{}","status":"completed"}}"#;

    assert!(matches!(
        codec.push_sse("response.output_item.done", first),
        Ok(events) if matches!(events.as_slice(), [ModelEvent::Called { call, .. }]
            if call.call_id.as_str() == "call_one")
    ));
    assert!(matches!(
        codec.push_sse("response.output_item.done", second),
        Err(plexmaton_provider::DecodeError::TooManyToolCalls { limit: 1 })
    ));
}
