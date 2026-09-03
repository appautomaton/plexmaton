use std::convert::Infallible;

use futures_util::stream;
use plexmaton_agent::{
    ModelEvent, ModelRequest, ProviderCodecId, ProviderReplay, RequestItem, StopReason,
};
use plexmaton_core::TokenUsage;
use plexmaton_provider::{
    DecodeLimits, OpenAiCodec, Protocol, SseDecodeError, drive_sse, encode_request,
};
use serde_json::Value;

#[path = "provider_fixtures/argument_boundaries.rs"]
mod argument_boundaries;
mod support;
#[path = "provider_fixtures/usage_boundaries.rs"]
mod usage_boundaries;

use support::{
    ResultTestExt, called, complete_answer, complete_tool_step, decode_fixture, open_agent,
    profile, read_tool, reasoning_text, reported_usage, visible_text,
};

const CHAT_TOOL_CALL: &str = include_str!("fixtures/chat_tool_call.sse");
const CHAT_FINAL_ANSWER: &str = include_str!("fixtures/chat_final_answer.sse");
const RESPONSES_TOOL_CALL: &str = include_str!("fixtures/responses_tool_call.sse");
const RESPONSES_FINAL_ANSWER: &str = include_str!("fixtures/responses_final_answer.sse");

#[tokio::test]
async fn prv_1_chat_fixture_drives_a_full_stateless_tool_round_trip() {
    let profile = profile(Protocol::ChatCompletions);
    let (mut agent, initial_request) = open_agent("What is this project called?");
    let initial_encoded = encode_request(&profile, &initial_request, &[read_tool()], None)
        .unwrap_or_else(|error| panic!("initial Chat request should encode: {error}"));
    assert_eq!(
        initial_encoded["messages"][0]["content"],
        "What is this project called?"
    );
    let first = decode_fixture(&profile, CHAT_TOOL_CALL, &[1, 2, 7, 31]).await;

    assert!(matches!(
        first.as_slice(),
        [
            ModelEvent::ReasoningDelta(_),
            ModelEvent::ReasoningDelta(_),
            ModelEvent::Called(_),
            ModelEvent::Usage(TokenUsage::Complete(_)),
            ModelEvent::Stopped(StopReason::ToolCalls)
        ]
    ));
    let call = called(&first);
    assert_eq!(call.name, "read_file");
    assert_eq!(call.arguments, r#"{"path":"README.md"}"#);
    assert!(matches!(
        reported_usage(&first),
        TokenUsage::Complete(counts)
            if counts.input == 12
                && counts.cached_input == Some(2)
                && counts.cache_write_input == Some(1)
                && counts.output == 5
                && counts.reasoning_output == Some(3)
                && counts.total == 17
    ));

    let request = complete_tool_step(&mut agent, &first, "Plexmaton");
    let encoded = encode_request(&profile, &request, &[read_tool()], Some(256))
        .unwrap_or_else(|error| panic!("Chat request should encode: {error}"));
    assert_eq!(encoded["model"], "gpt-5.6-luna");
    assert_eq!(encoded["reasoning_effort"], "high");
    assert_eq!(encoded["stream_options"]["include_usage"], true);
    assert_eq!(encoded["max_completion_tokens"], 256);
    assert_eq!(encoded["messages"][1]["reasoning_content"], "Need README.");
    assert_eq!(encoded["messages"][1]["tool_calls"][0]["id"], "call_read_1");
    assert_eq!(encoded["messages"][2]["tool_call_id"], "call_read_1");
    assert_eq!(encoded["tools"][0]["function"]["strict"], true);

    let second = decode_fixture(&profile, CHAT_FINAL_ANSWER, &[3, 5, 11, 29]).await;
    complete_answer(&mut agent, &second);
    assert_eq!(visible_text(&second), "Plexmaton.");
    assert_eq!(reasoning_text(&second), "The file names the project. ");
    assert!(matches!(
        reported_usage(&second),
        TokenUsage::Complete(counts) if counts.total == 28
    ));
    assert!(matches!(
        second.last(),
        Some(ModelEvent::Stopped(StopReason::EndOfTurn))
    ));
}

#[tokio::test]
async fn prv_3_responses_fixture_replays_encrypted_reasoning_exactly_and_round_trips_tools() {
    let profile = profile(Protocol::Responses);
    let (mut agent, initial_request) = open_agent("What is this project called?");
    let initial_encoded = encode_request(&profile, &initial_request, &[read_tool()], None)
        .unwrap_or_else(|error| panic!("initial Responses request should encode: {error}"));
    assert_eq!(
        initial_encoded["input"][0]["content"],
        "What is this project called?"
    );
    let first = decode_fixture(&profile, RESPONSES_TOOL_CALL, &[1, 4, 13, 37]).await;

    assert!(matches!(
        first.as_slice(),
        [
            ModelEvent::Replay(_),
            ModelEvent::Called(_),
            ModelEvent::Usage(TokenUsage::Complete(_)),
            ModelEvent::Stopped(StopReason::ToolCalls)
        ]
    ));
    let streamed_replay = first
        .iter()
        .find_map(|event| match event {
            ModelEvent::Replay(replay) => Some(replay.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("fixture should retain encrypted reasoning"));
    let replay_item: Value = serde_json::from_str(streamed_replay.payload())
        .unwrap_or_else(|error| panic!("replay should be JSON: {error}"));
    assert_eq!(replay_item["encrypted_content"], "enc_fixture_1_🔐");
    assert_eq!(replay_item["summary"][0]["text"], "Need README.");

    let call = called(&first);
    assert_eq!(call.name, "read_file");
    assert_eq!(call.arguments, r#"{"path":"README.md"}"#);
    assert!(matches!(
        reported_usage(&first),
        TokenUsage::Complete(counts)
            if counts.input == 14
                && counts.cached_input == Some(3)
                && counts.cache_write_input == Some(1)
                && counts.output == 6
                && counts.reasoning_output == Some(4)
                && counts.total == 20
    ));
    let request = complete_tool_step(&mut agent, &first, "Plexmaton");
    let recorded_replay = request
        .items
        .iter()
        .find_map(|item| match item {
            RequestItem::ProviderReplay(replay) => Some(replay),
            _ => None,
        })
        .unwrap_or_else(|| panic!("agent record should retain provider replay"));
    assert_eq!(recorded_replay, &streamed_replay);
    let encoded = encode_request(&profile, &request, &[read_tool()], Some(256))
        .unwrap_or_else(|error| panic!("Responses request should encode: {error}"));
    assert_eq!(encoded["store"], false);
    assert_eq!(encoded["include"][0], "reasoning.encrypted_content");
    assert!(encoded.get("previous_response_id").is_none());
    assert_eq!(encoded["input"][1], replay_item);
    assert_eq!(encoded["input"][2]["call_id"], "call_read_1");
    assert_eq!(encoded["input"][3]["type"], "function_call_output");
    assert_eq!(encoded["tools"][0]["strict"], true);

    let second = decode_fixture(&profile, RESPONSES_FINAL_ANSWER, &[2, 3, 17, 41]).await;
    complete_answer(&mut agent, &second);
    assert!(matches!(second.first(), Some(ModelEvent::Replay(_))));
    assert_eq!(visible_text(&second), "Plexmaton.");
    assert!(matches!(
        reported_usage(&second),
        TokenUsage::Complete(counts) if counts.total == 31
    ));
    assert!(matches!(
        second.last(),
        Some(ModelEvent::Stopped(StopReason::EndOfTurn))
    ));
}

#[test]
fn prv_1_protocol_selection_never_falls_back_across_replay_grammars() {
    let replay = ProviderReplay::new(
        ProviderCodecId::new("openai_responses")
            .unwrap_or_else(|error| panic!("fixture codec: {error:?}")),
        r#"{"type":"reasoning","encrypted_content":"ciphertext"}"#.to_owned(),
    )
    .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
    let request = ModelRequest {
        items: vec![RequestItem::ProviderReplay(replay)],
    };

    assert!(matches!(
        encode_request(&profile(Protocol::ChatCompletions), &request, &[], None),
        Err(plexmaton_provider::EncodeError::OpaqueReplayInChat)
    ));
    let request = ModelRequest {
        items: vec![RequestItem::Reasoning {
            text: "plaintext".to_owned(),
        }],
    };
    assert!(matches!(
        encode_request(&profile(Protocol::Responses), &request, &[], None),
        Err(plexmaton_provider::EncodeError::PlainReasoningInResponses)
    ));
}

#[tokio::test]
async fn prv_2_and_prv_7_reject_unbounded_or_incomplete_provider_input() {
    let profile = profile(Protocol::ChatCompletions);
    let mut limits = DecodeLimits::for_profile(&profile);
    limits.max_retained_output_bytes = 3;
    let source = stream::iter([Ok::<_, Infallible>(CHAT_FINAL_ANSWER.as_bytes())]);
    let error = drive_sse(profile.protocol(), source, limits, |_| {
        std::future::ready(())
    })
    .await
    .unwrap_err_or_else();
    assert!(matches!(
        error,
        SseDecodeError::Decode(plexmaton_provider::DecodeError::RetainedOutputTooLarge {
            limit: 3
        })
    ));

    let incomplete = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n";
    let source = stream::iter([Ok::<_, Infallible>(incomplete.as_bytes())]);
    let error = drive_sse(
        profile.protocol(),
        source,
        DecodeLimits::for_profile(&profile),
        |_| std::future::ready(()),
    )
    .await
    .unwrap_err_or_else();
    assert!(matches!(
        error,
        SseDecodeError::Decode(plexmaton_provider::DecodeError::IncompleteStream)
    ));
}

#[tokio::test]
async fn prv_2_stopped_is_withheld_until_the_stream_trailer_is_valid() {
    let profile = profile(Protocol::ChatCompletions);
    let missing_done = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    );
    let source = stream::iter([Ok::<_, Infallible>(missing_done.as_bytes())]);
    let mut emitted = Vec::new();

    let error = drive_sse(
        profile.protocol(),
        source,
        DecodeLimits::for_profile(&profile),
        |event| {
            emitted.push(event);
            std::future::ready(())
        },
    )
    .await
    .unwrap_err_or_else();

    assert!(matches!(
        error,
        SseDecodeError::Decode(plexmaton_provider::DecodeError::IncompleteStream)
    ));
    assert_eq!(emitted, [ModelEvent::TextDelta("hi".to_owned())]);
}

#[test]
fn prv_2_rejects_tool_arguments_before_they_can_reach_admission() {
    let profile = profile(Protocol::ChatCompletions);
    let mut limits = DecodeLimits::for_profile(&profile);
    limits.max_tool_argument_bytes = 4;
    let mut codec = OpenAiCodec::new(profile.protocol(), limits);
    let event = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"12345"}}]},"finish_reason":null}]}"#;

    assert!(matches!(
        codec.push_sse("message", event),
        Err(plexmaton_provider::DecodeError::ToolArgumentsTooLarge { index: 0, limit: 4 })
    ));
}

#[test]
fn prv_3_rejects_opaque_replay_before_step_state_can_grow() {
    let profile = profile(Protocol::Responses);
    let mut limits = DecodeLimits::for_profile(&profile);
    limits.max_replay_bytes = 8;
    let mut codec = OpenAiCodec::new(profile.protocol(), limits);
    let event = r#"{"type":"response.output_item.done","output_index":0,"item":{"id":"rs_1","type":"reasoning","summary":[],"encrypted_content":"ciphertext","status":"completed"}}"#;

    assert!(matches!(
        codec.push_sse("response.output_item.done", event),
        Err(plexmaton_provider::DecodeError::RetainedReplayTooLarge { limit: 8 })
    ));
}

#[test]
fn prv_2_bounds_even_empty_responses_output_items() {
    let profile = profile(Protocol::Responses);
    let mut limits = DecodeLimits::for_profile(&profile);
    limits.max_output_items = 1;
    let mut codec = OpenAiCodec::new(profile.protocol(), limits);
    let first = r#"{"type":"response.output_item.done","output_index":0,"item":{"id":"msg_1","type":"message","role":"assistant","content":[],"status":"completed"}}"#;
    let second = r#"{"type":"response.output_item.done","output_index":1,"item":{"id":"msg_2","type":"message","role":"assistant","content":[],"status":"completed"}}"#;

    assert!(codec.push_sse("response.output_item.done", first).is_ok());
    assert!(matches!(
        codec.push_sse("response.output_item.done", second),
        Err(plexmaton_provider::DecodeError::TooManyOutputItems { limit: 1 })
    ));
}

#[test]
fn prv_2_responses_text_done_confirms_deltas_or_supplies_the_only_copy() {
    let profile = profile(Protocol::Responses);
    let limits = DecodeLimits::for_profile(&profile);
    let mut done_only = OpenAiCodec::new(profile.protocol(), limits);
    let done =
        r#"{"type":"response.output_text.done","output_index":0,"content_index":0,"text":"whole"}"#;
    assert!(matches!(
        done_only.push_sse("response.output_text.done", done),
        Ok(events) if events == [ModelEvent::TextDelta("whole".to_owned())]
    ));

    let mut conflicting = OpenAiCodec::new(profile.protocol(), limits);
    let delta = r#"{"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"part"}"#;
    conflicting
        .push_sse("response.output_text.delta", delta)
        .unwrap_or_else(|error| panic!("fixture delta should decode: {error}"));
    assert!(matches!(
        conflicting.push_sse("response.output_text.done", done),
        Err(plexmaton_provider::DecodeError::ConflictingOutputText {
            output_index: 0,
            content_index: 0,
            field: "text"
        })
    ));
}

#[test]
fn prv_5_responses_done_only_refusal_is_visible_and_typed() {
    let profile = profile(Protocol::Responses);
    let mut codec = OpenAiCodec::new(profile.protocol(), DecodeLimits::for_profile(&profile));
    let refusal = r#"{"type":"response.refusal.done","output_index":0,"content_index":0,"refusal":"declined"}"#;
    assert!(matches!(
        codec.push_sse("response.refusal.done", refusal),
        Ok(events) if events == [ModelEvent::TextDelta("declined".to_owned())]
    ));
    let completed = r#"{"type":"response.completed","response":{"status":"completed"}}"#;
    assert!(matches!(
        codec.push_sse("response.completed", completed),
        Ok(events)
            if events == [
                ModelEvent::Usage(TokenUsage::Unavailable),
                ModelEvent::Stopped(StopReason::Refused),
            ]
    ));
    codec
        .finish()
        .unwrap_or_else(|error| panic!("typed refusal should complete: {error}"));
}

#[tokio::test]
async fn prv_2_sse_framing_rejects_partial_utf8() {
    let profile = profile(Protocol::Responses);
    let source = stream::iter([Ok::<_, Infallible>(vec![
        b'd', b'a', b't', b'a', b':', b' ', 0xff, b'\n', b'\n',
    ])]);
    let error = drive_sse(
        profile.protocol(),
        source,
        DecodeLimits::for_profile(&profile),
        |_| std::future::ready(()),
    )
    .await
    .unwrap_err_or_else();
    assert!(matches!(error, SseDecodeError::InvalidUtf8));
}
