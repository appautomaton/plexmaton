use std::convert::Infallible;

use futures_util::stream;
use plexmaton_agent::ModelEvent;
use plexmaton_provider::{
    DecodeLimits, OpenAiCodec, Protocol, ProviderConfig, ProviderProfile, SseDecodeError, drive_sse,
};

const CHAT_DUPLICATE_TOOL_CALL_ID: &str = include_str!("fixtures/chat_duplicate_tool_call_id.sse");
const RESPONSES_DUPLICATE_TOOL_CALL_ID: &str =
    include_str!("fixtures/responses_duplicate_tool_call_id.sse");

fn profile(protocol: Protocol) -> ProviderProfile {
    let protocol = match protocol {
        Protocol::Responses => "responses",
        Protocol::ChatCompletions => "chat_completions",
    };
    ProviderConfig::parse(&format!(
        r#"
active_provider = "test"
[providers.test]
kind = "openai_compatible"
protocol = "{protocol}"
base_url = "http://127.0.0.1:8317/v1"
model = "fixture"
api_key_env = "TEST_KEY"
reasoning_effort = "low"
"#
    ))
    .unwrap_or_else(|error| panic!("fixture profile: {error}"))
    .active()
    .clone()
}

#[tokio::test]
async fn prv_2_rejects_duplicate_chat_tool_call_ids_before_emission() {
    let profile = profile(Protocol::ChatCompletions);
    let source = stream::iter([Ok::<_, Infallible>(CHAT_DUPLICATE_TOOL_CALL_ID.as_bytes())]);
    let mut emitted = Vec::new();

    let result = drive_sse(
        profile.protocol(),
        source,
        DecodeLimits::for_profile(&profile),
        |event| {
            emitted.push(event);
            std::future::ready(())
        },
    )
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
    let profile = profile(Protocol::Responses);
    let source = stream::iter([Ok::<_, Infallible>(
        RESPONSES_DUPLICATE_TOOL_CALL_ID.as_bytes(),
    )]);
    let mut emitted = Vec::new();

    let result = drive_sse(
        profile.protocol(),
        source,
        DecodeLimits::for_profile(&profile),
        |event| {
            emitted.push(event);
            std::future::ready(())
        },
    )
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
    assert!(matches!(emitted.as_slice(), [ModelEvent::Called(call)]
        if call.call_id.as_str() == "call_duplicate" && call.arguments.contains("one")));
}

#[test]
fn prv_2_responses_counts_incrementally_completed_calls_toward_the_step_bound() {
    let profile = profile(Protocol::Responses);
    let mut limits = DecodeLimits::for_profile(&profile);
    limits.max_tool_calls = 1;
    let mut codec = OpenAiCodec::new(profile.protocol(), limits);
    let first = r#"{"type":"response.output_item.done","output_index":0,"item":{"id":"fc_one","type":"function_call","call_id":"call_one","name":"read_file","arguments":"{}","status":"completed"}}"#;
    let second = r#"{"type":"response.output_item.done","output_index":1,"item":{"id":"fc_two","type":"function_call","call_id":"call_two","name":"read_file","arguments":"{}","status":"completed"}}"#;

    assert!(matches!(
        codec.push_sse("response.output_item.done", first),
        Ok(events) if matches!(events.as_slice(), [ModelEvent::Called(call)]
            if call.call_id.as_str() == "call_one")
    ));
    assert!(matches!(
        codec.push_sse("response.output_item.done", second),
        Err(plexmaton_provider::DecodeError::TooManyToolCalls { limit: 1 })
    ));
}
