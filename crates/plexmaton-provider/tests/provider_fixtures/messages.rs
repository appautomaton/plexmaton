use super::*;
use plexmaton_provider::DecodeError;
use serde_json::json;

const TOOL: &str = include_str!("../fixtures/messages_tool_call.sse");
const ANSWER: &str = include_str!("../fixtures/messages_final_answer.sse");

/// PRV-5: a server-tool continuation is a known unsupported surface, never a local-tool stop.
#[test]
fn prv_5_messages_pause_turn_is_explicitly_unsupported() {
    let model = profile(ModelApi::AnthropicMessages);
    let scope = plexmaton_agent::RequestAttemptId::new("pause-turn").expect("scope");
    let mut codec = ProviderCodec::new(&scope, &model, DecodeLimits::production());
    codec.push_sse("message_start", r#"{"type":"message_start","message":{"role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}"#).expect("start");
    assert!(
        matches!(codec.push_sse("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"pause_turn"},"usage":{"output_tokens":1}}"#), Err(DecodeError::UnsupportedEvent(kind)) if kind == "messages_pause_turn")
    );
}

/// PRV-1/PRV-3/TIM-3: Messages uses the same agent loop, exact signatures and inclusive input.
#[tokio::test]
async fn prv_1_messages_signed_tool_round_trip_normalizes_cumulative_usage() {
    let model = profile(ModelApi::AnthropicMessages);
    let (mut agent, request) = open_agent("Read the file.");
    let body = encode_request(&model, &request, &[read_tool()], None)
        .unwrap_or_else(|error| panic!("request: {error}"));
    assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
    let first = decode_fixture(&model, TOOL, &[1, 4, 13]).await;
    let usage = first
        .iter()
        .filter_map(|event| match event {
            ModelEvent::Usage(usage) => Some(usage),
            _ => None,
        })
        .next_back();
    assert!(
        matches!(usage, Some(TokenUsage::Complete(counts)) if counts.input == 150 && counts.output == 9 && counts.reasoning_output == Some(5) && counts.total == 159)
    );
    let request = complete_tool_step(&mut agent, &first, "Plexmaton");
    let body = encode_request(&model, &request, &[read_tool()], None)
        .unwrap_or_else(|error| panic!("replay: {error}"));
    assert_eq!(
        body["messages"][1]["content"][0],
        json!({"type":"thinking","thinking":"Need README.","signature":"signed-fixture"})
    );
    assert_eq!(
        body["messages"][1]["content"][1],
        json!({"type":"redacted_thinking","data":"redacted-fixture"})
    );
    assert_eq!(body["messages"][1]["content"][2]["type"], "tool_use");
    assert_eq!(body["messages"][2]["role"], "user");
    assert_eq!(
        body["messages"][2]["content"][0]["tool_use_id"],
        "toolu_read"
    );
    assert_eq!(body["messages"][2]["content"][0]["is_error"], false);
    let second = decode_fixture(&model, ANSWER, &[2, 17, 9]).await;
    complete_answer(&mut agent, &second);
    assert_eq!(visible_text(&second), "Plexmaton.");
    assert!(!agent.is_running());
}

/// PRV-2/PRV-3/PRV-5: no unfinished call or signature can become a completed model step.
#[test]
fn prv_2_messages_rejects_unfinished_mismatched_and_oversized_blocks() {
    let model = profile(ModelApi::AnthropicMessages);
    let start = json!({"type":"message_start","message":{"role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":0}}});
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &model,
        DecodeLimits::production(),
    );
    codec
        .push_sse("message_start", &start.to_string())
        .unwrap_or_else(|error| panic!("start: {error}"));
    let thinking = json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}});
    codec
        .push_sse("content_block_start", &thinking.to_string())
        .unwrap_or_else(|error| panic!("thinking: {error}"));
    assert!(
        codec
            .push_sse(
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#
            )
            .is_err()
    );

    let mut limits = DecodeLimits::production();
    limits.max_tool_argument_bytes = 4;
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &model,
        limits,
    );
    codec
        .push_sse("message_start", &start.to_string())
        .unwrap_or_else(|error| panic!("start: {error}"));
    codec.push_sse("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call","name":"read_file","input":{}}}"#).unwrap_or_else(|error| panic!("tool: {error}"));
    assert!(matches!(codec.push_sse("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"12345"}}"#), Err(DecodeError::ToolArgumentsTooLarge { limit:4,.. })));
    assert!(codec.finish().is_err());

    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &model,
        DecodeLimits::production(),
    );
    assert!(matches!(
        codec.push_sse("content_block_delta", &start.to_string()),
        Err(DecodeError::ConflictingEventType { .. })
    ));
}

/// PRV-5/TIM-3: context limits and refusals preserve the usage carried by the final message delta.
#[tokio::test]
async fn prv_5_messages_context_limits_and_refusals_keep_final_usage() {
    let model = profile(ModelApi::AnthropicMessages);
    for (wire, expected) in [
        ("model_context_window_exceeded", StopReason::ContextLimit),
        ("refusal", StopReason::Refused),
    ] {
        let fixture = ANSWER.replace("end_turn", wire);
        let events = decode_fixture(&model, &fixture, &[7, 3]).await;
        assert!(matches!(events.last(),Some(ModelEvent::Stopped(reason)) if *reason == expected));
        assert!(events.iter().rev().any(|event| matches!(event,ModelEvent::Usage(TokenUsage::Complete(counts)) if counts.output == 7 && counts.total == 207)));
    }
}
