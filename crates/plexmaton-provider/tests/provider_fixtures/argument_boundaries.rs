use plexmaton_agent::MAX_REQUESTED_TOOL_ARGUMENT_BYTES;
use plexmaton_provider::{DecodeError, DecodeLimits, OpenAiCodec, Protocol};
use serde_json::json;

use super::support::profile;

/// PRV-2: canonical admission reserve never widens either provider's raw call boundary.
#[test]
fn prv_2_production_tool_argument_limit_remains_64_kibibytes() {
    assert_eq!(MAX_REQUESTED_TOOL_ARGUMENT_BYTES, 64 * 1024);
    for protocol in [Protocol::ChatCompletions, Protocol::Responses] {
        let profile = profile(protocol);
        let limits = DecodeLimits::for_profile(&profile);
        assert_eq!(
            limits.max_tool_argument_bytes,
            MAX_REQUESTED_TOOL_ARGUMENT_BYTES
        );
        let mut codec = OpenAiCodec::new(protocol, limits);
        match protocol {
            Protocol::ChatCompletions => check_chat(&mut codec),
            Protocol::Responses => check_responses(&mut codec),
        }
    }
}

fn check_chat(codec: &mut OpenAiCodec) {
    let at_limit = json!({
        "choices": [{
            "index": 0,
            "delta": {"tool_calls": [{
                "index": 0,
                "id": "call_1",
                "function": {"name": "read_file", "arguments": "x".repeat(MAX_REQUESTED_TOOL_ARGUMENT_BYTES)}
            }]},
            "finish_reason": null
        }]
    })
    .to_string();
    assert!(codec.push_sse("message", &at_limit).is_ok());
    let over_limit = json!({
        "choices": [{
            "index": 0,
            "delta": {"tool_calls": [{"index": 0, "function": {"arguments": "x"}}]},
            "finish_reason": null
        }]
    })
    .to_string();
    assert_too_large(codec.push_sse("message", &over_limit));
}

fn check_responses(codec: &mut OpenAiCodec) {
    let added = json!({
        "type": "response.output_item.added",
        "output_index": 0,
        "item": {"id": "fc_1", "type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": ""}
    })
    .to_string();
    assert!(codec.push_sse("response.output_item.added", &added).is_ok());
    let at_limit = json!({
        "type": "response.function_call_arguments.delta",
        "item_id": "fc_1",
        "output_index": 0,
        "delta": "x".repeat(MAX_REQUESTED_TOOL_ARGUMENT_BYTES)
    })
    .to_string();
    assert!(
        codec
            .push_sse("response.function_call_arguments.delta", &at_limit)
            .is_ok()
    );
    let over_limit = json!({
        "type": "response.function_call_arguments.delta",
        "item_id": "fc_1",
        "output_index": 0,
        "delta": "x"
    })
    .to_string();
    assert_too_large(codec.push_sse("response.function_call_arguments.delta", &over_limit));
}

fn assert_too_large(result: Result<Vec<plexmaton_agent::ModelEvent>, DecodeError>) {
    assert!(matches!(
        result,
        Err(DecodeError::ToolArgumentsTooLarge {
            index: 0,
            limit: MAX_REQUESTED_TOOL_ARGUMENT_BYTES
        })
    ));
}
