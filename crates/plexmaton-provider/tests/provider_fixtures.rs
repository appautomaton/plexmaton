use std::convert::Infallible;

use futures_util::stream;
use plexmaton_agent::{
    AssistantBlock, AssistantOutput, AssistantReplay, ContextAtom, ContextAtomValue, ModelEvent,
    ModelOutputPosition, ModelRequest, ProviderCodecId, ProviderCodecRevision,
    ProviderModelFamilyId, ProviderReplay, ProviderReplayOwnerId, ReplayCompatibility, StopReason,
    ToolBatch, ToolBatchResult, ToolCall, ToolOutcome,
};
use plexmaton_core::{SessionEntryId, TokenUsage, ToolCallId, TranscriptItemId};
use plexmaton_provider::{
    DecodeLimits, ModelApi, ProviderCodec, SseDecodeError, drive_sse, encode_request,
};
use serde_json::Value;

#[path = "provider_fixtures/argument_boundaries.rs"]
mod argument_boundaries;
#[path = "provider_fixtures/gemini.rs"]
mod gemini;
#[path = "provider_fixtures/messages.rs"]
mod messages;
#[path = "provider_fixtures/request_options.rs"]
mod request_options;
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
    let profile = profile(ModelApi::OpenaiChatCompletions);
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
            ModelEvent::ReasoningDelta { .. },
            ModelEvent::ReasoningDelta { .. },
            ModelEvent::Called { .. },
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
    let profile = profile(ModelApi::OpenaiResponses);
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
            ModelEvent::ReasoningDelta { .. },
            ModelEvent::Replay { .. },
            ModelEvent::Called { .. },
            ModelEvent::Replay { .. },
            ModelEvent::Usage(TokenUsage::Complete(_)),
            ModelEvent::Stopped(StopReason::ToolCalls)
        ]
    ));
    let streamed_replay = first
        .iter()
        .find_map(|event| match event {
            ModelEvent::Replay { replay, .. } => Some(replay.clone()),
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
        .atoms
        .iter()
        .find_map(|atom| match atom.value() {
            ContextAtomValue::ToolBatch(batch) => batch.assistant().replay(),
            _ => None,
        })
        .unwrap_or_else(|| panic!("agent record should retain provider replay"));
    assert_eq!(
        recorded_replay.compatible_with(),
        streamed_replay.compatible_with()
    );
    assert_eq!(recorded_replay.attachments().len(), 2);
    assert_eq!(recorded_replay.attachments()[1].block(), 1);
    assert_eq!(
        recorded_replay.attachments()[0].payload(),
        streamed_replay.payload()
    );
    let encoded = encode_request(&profile, &request, &[read_tool()], Some(256))
        .unwrap_or_else(|error| panic!("Responses request should encode: {error}"));
    assert_eq!(encoded["store"], false);
    assert_eq!(encoded["include"][0], "reasoning.encrypted_content");
    assert!(encoded.get("previous_response_id").is_none());
    assert_eq!(encoded["input"][1], replay_item);
    assert_eq!(encoded["input"][2]["call_id"], "call_read_1");
    assert_eq!(encoded["input"][2]["id"], "fc_fixture_1");
    assert_eq!(encoded["input"][2]["status"], "completed");
    assert_eq!(encoded["input"][3]["type"], "function_call_output");
    assert_eq!(encoded["tools"][0]["strict"], true);

    let second = decode_fixture(&profile, RESPONSES_FINAL_ANSWER, &[2, 3, 17, 41]).await;
    complete_answer(&mut agent, &second);
    assert!(matches!(
        second.as_slice(),
        [
            ModelEvent::ReasoningDelta { .. },
            ModelEvent::Replay { .. },
            ModelEvent::TextDelta { .. },
            ModelEvent::TextDelta { .. },
            ModelEvent::Replay { .. },
            ModelEvent::Usage(_),
            ModelEvent::Stopped(StopReason::EndOfTurn),
        ]
    ));
    assert_eq!(visible_text(&second), "Plexmaton.");
    assert_eq!(reasoning_text(&second), "The file names the project.");
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
    let chat = profile(ModelApi::OpenaiChatCompletions);
    let replay = ProviderReplay::new(
        chat.replay_compatibility(),
        r#"{"type":"reasoning","encrypted_content":"ciphertext"}"#.to_owned(),
    )
    .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
    let replay = AssistantReplay::from_positioned([(0, replay)])
        .unwrap_or_else(|error| panic!("fixture assistant replay: {error}"));
    let output = AssistantOutput::new(
        vec![AssistantBlock::Reasoning {
            item_id: transcript_item("opaque-reasoning"),
            text: String::new(),
        }],
        replay,
    )
    .unwrap_or_else(|error| panic!("fixture assistant output: {error}"));
    let request = ModelRequest {
        session_id: plexmaton_core::SessionId::new("fixture-session")
            .unwrap_or_else(|error| panic!("session: {error}")),
        atoms: vec![
            ContextAtom::assistant(session_entry("opaque-output"), output)
                .unwrap_or_else(|error| panic!("fixture context atom: {error}")),
        ],
    };

    assert!(matches!(
        encode_request(&chat, &request, &[], None),
        Err(plexmaton_provider::EncodeError::OpaqueReplayInChat)
    ));
    let output = AssistantOutput::new(
        vec![AssistantBlock::Reasoning {
            item_id: transcript_item("plain-reasoning"),
            text: "plaintext".to_owned(),
        }],
        None,
    )
    .unwrap_or_else(|error| panic!("fixture assistant output: {error}"));
    let request = ModelRequest {
        session_id: plexmaton_core::SessionId::new("fixture-session")
            .unwrap_or_else(|error| panic!("session: {error}")),
        atoms: vec![
            ContextAtom::assistant(session_entry("plain-output"), output)
                .unwrap_or_else(|error| panic!("fixture context atom: {error}")),
        ],
    };
    assert_eq!(
        encode_request(&profile(ModelApi::OpenaiResponses), &request, &[], None)
            .expect("unsigned summaries are not fabricated into replay")["input"],
        serde_json::json!([])
    );
}

#[test]
fn prv_1_both_protocols_preserve_parallel_call_and_result_order() {
    let calls = [
        tool_call("call_first", "first_file"),
        tool_call("call_second", "second_file"),
    ];
    let output = AssistantOutput::new(
        vec![
            AssistantBlock::Text {
                item_id: transcript_item("preface"),
                text: "Checking both files.".to_owned(),
            },
            AssistantBlock::ToolCall {
                item_id: transcript_item("first-call"),
                call: calls[0].clone(),
            },
            AssistantBlock::ToolCall {
                item_id: transcript_item("second-call"),
                call: calls[1].clone(),
            },
        ],
        None,
    )
    .unwrap_or_else(|error| panic!("fixture assistant output: {error}"));
    let batch = ToolBatch::new(
        output,
        vec![
            ToolBatchResult::new(
                calls[0].call_id.clone(),
                ToolOutcome::Succeeded {
                    output: "first result".to_owned(),
                },
            ),
            ToolBatchResult::new(
                calls[1].call_id.clone(),
                ToolOutcome::Succeeded {
                    output: "second result".to_owned(),
                },
            ),
        ],
    )
    .unwrap_or_else(|error| panic!("fixture tool batch: {error}"));
    let request = ModelRequest {
        session_id: plexmaton_core::SessionId::new("fixture-session")
            .unwrap_or_else(|error| panic!("session: {error}")),
        atoms: vec![
            ContextAtom::tool_batch(vec![session_entry("parallel-batch")], batch)
                .unwrap_or_else(|error| panic!("fixture context atom: {error}")),
        ],
    };

    let chat = encode_request(
        &profile(ModelApi::OpenaiChatCompletions),
        &request,
        &[],
        None,
    )
    .unwrap_or_else(|error| panic!("ordered Chat batch should encode: {error}"));
    assert_eq!(chat["messages"][0]["tool_calls"][0]["id"], "call_first");
    assert_eq!(chat["messages"][0]["tool_calls"][1]["id"], "call_second");
    assert_eq!(chat["messages"][1]["tool_call_id"], "call_first");
    assert_eq!(chat["messages"][2]["tool_call_id"], "call_second");

    let responses = encode_request(&profile(ModelApi::OpenaiResponses), &request, &[], None)
        .unwrap_or_else(|error| panic!("ordered Responses batch should encode: {error}"));
    assert_eq!(responses["input"][1]["call_id"], "call_first");
    assert_eq!(responses["input"][2]["call_id"], "call_second");
    assert_eq!(responses["input"][3]["call_id"], "call_first");
    assert_eq!(responses["input"][4]["call_id"], "call_second");
}

#[test]
fn prv_1_chat_refuses_cross_kind_order_its_wire_cannot_represent() {
    let call = tool_call("call_middle", "middle_file");
    let output = AssistantOutput::new(
        vec![
            AssistantBlock::Text {
                item_id: transcript_item("before"),
                text: "before".to_owned(),
            },
            AssistantBlock::ToolCall {
                item_id: transcript_item("middle"),
                call: call.clone(),
            },
            AssistantBlock::Text {
                item_id: transcript_item("after"),
                text: "after".to_owned(),
            },
        ],
        None,
    )
    .unwrap_or_else(|error| panic!("fixture assistant output: {error}"));
    let batch = ToolBatch::new(
        output,
        vec![ToolBatchResult::new(
            call.call_id,
            ToolOutcome::Succeeded {
                output: "result".to_owned(),
            },
        )],
    )
    .unwrap_or_else(|error| panic!("fixture tool batch: {error}"));
    let request = ModelRequest {
        session_id: plexmaton_core::SessionId::new("fixture-session")
            .unwrap_or_else(|error| panic!("session: {error}")),
        atoms: vec![
            ContextAtom::tool_batch(vec![session_entry("cross-kind")], batch)
                .unwrap_or_else(|error| panic!("fixture context atom: {error}")),
        ],
    };

    assert!(matches!(
        encode_request(
            &profile(ModelApi::OpenaiChatCompletions),
            &request,
            &[],
            None,
        ),
        Err(plexmaton_provider::EncodeError::UnrepresentableChatOrder)
    ));
    let responses = encode_request(&profile(ModelApi::OpenaiResponses), &request, &[], None)
        .unwrap_or_else(|error| panic!("Responses preserves cross-kind order: {error}"));
    let types: Vec<_> = responses["input"]
        .as_array()
        .unwrap_or_else(|| panic!("Responses input is an array"))
        .iter()
        .filter_map(|item| item["type"].as_str())
        .collect();
    assert_eq!(
        types,
        [
            "message",
            "function_call",
            "message",
            "function_call_output"
        ]
    );
}

#[test]
fn prv_3_replay_compatibility_covers_route_codec_revision_and_model_family() {
    let selected = profile(ModelApi::OpenaiResponses);
    let expected = selected.replay_compatibility();
    let cases = [
        ReplayCompatibility::new(
            replay_owner("another-route"),
            expected.codec().clone(),
            expected.codec_revision(),
            expected.model_family().clone(),
        ),
        ReplayCompatibility::new(
            expected.owner().clone(),
            replay_codec("another-codec"),
            expected.codec_revision(),
            expected.model_family().clone(),
        ),
        ReplayCompatibility::new(
            expected.owner().clone(),
            expected.codec().clone(),
            ProviderCodecRevision::new(expected.codec_revision().get() + 1)
                .unwrap_or_else(|error| panic!("fixture codec revision: {error:?}")),
            expected.model_family().clone(),
        ),
        ReplayCompatibility::new(
            expected.owner().clone(),
            expected.codec().clone(),
            expected.codec_revision(),
            replay_model_family("another-model-family"),
        ),
    ];

    for incompatible in cases {
        let request = opaque_replay_request(incompatible.clone());
        assert!(matches!(
            encode_request(&selected, &request, &[], None),
            Err(plexmaton_provider::EncodeError::IncompatibleReplay { found, expected: actual })
                if *found == incompatible && *actual == expected
        ));
    }
}

#[tokio::test]
async fn prv_2_and_prv_7_reject_unbounded_or_incomplete_provider_input() {
    let profile = profile(ModelApi::OpenaiChatCompletions);
    let mut limits = DecodeLimits::production();
    limits.max_retained_output_bytes = 3;
    let source = stream::iter([Ok::<_, Infallible>(CHAT_FINAL_ANSWER.as_bytes())]);
    let error = drive_sse(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        source,
        limits,
        |_| std::future::ready(()),
    )
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
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        source,
        DecodeLimits::production(),
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
    let profile = profile(ModelApi::OpenaiChatCompletions);
    let missing_done = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    );
    let source = stream::iter([Ok::<_, Infallible>(missing_done.as_bytes())]);
    let mut emitted = Vec::new();

    let error = drive_sse(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        source,
        DecodeLimits::production(),
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
    assert_eq!(
        emitted,
        [ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 1),
            delta: "hi".to_owned(),
        }]
    );
}

#[test]
fn prv_2_rejects_tool_arguments_before_they_can_reach_admission() {
    let profile = profile(ModelApi::OpenaiChatCompletions);
    let mut limits = DecodeLimits::production();
    limits.max_tool_argument_bytes = 4;
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        limits,
    );
    let event = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"12345"}}]},"finish_reason":null}]}"#;

    assert!(matches!(
        codec.push_sse("message", event),
        Err(plexmaton_provider::DecodeError::ToolArgumentsTooLarge { index: 0, limit: 4 })
    ));
}

#[test]
fn prv_3_rejects_opaque_replay_before_step_state_can_grow() {
    let profile = profile(ModelApi::OpenaiResponses);
    let mut limits = DecodeLimits::production();
    limits.max_replay_bytes = 8;
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        limits,
    );
    let event = r#"{"type":"response.output_item.done","output_index":0,"item":{"id":"rs_1","type":"reasoning","summary":[],"encrypted_content":"ciphertext","status":"completed"}}"#;

    assert!(matches!(
        codec.push_sse("response.output_item.done", event),
        Err(plexmaton_provider::DecodeError::RetainedReplayTooLarge { limit: 8 })
    ));
}

#[test]
fn prv_2_bounds_even_empty_responses_output_items() {
    let profile = profile(ModelApi::OpenaiResponses);
    let mut limits = DecodeLimits::production();
    limits.max_output_items = 1;
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        limits,
    );
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
    let profile = profile(ModelApi::OpenaiResponses);
    let limits = DecodeLimits::production();
    let mut done_only = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        limits,
    );
    let done =
        r#"{"type":"response.output_text.done","output_index":0,"content_index":0,"text":"whole"}"#;
    assert!(matches!(
        done_only.push_sse("response.output_text.done", done),
        Ok(events) if events == [ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 0),
            delta: "whole".to_owned(),
        }]
    ));

    let mut conflicting = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        limits,
    );
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
    let profile = profile(ModelApi::OpenaiResponses);
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        DecodeLimits::production(),
    );
    let refusal = r#"{"type":"response.refusal.done","output_index":0,"content_index":0,"refusal":"declined"}"#;
    assert!(matches!(
        codec.push_sse("response.refusal.done", refusal),
        Ok(events) if events == [ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 0),
            delta: "declined".to_owned(),
        }]
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
    let profile = profile(ModelApi::OpenaiResponses);
    let source = stream::iter([Ok::<_, Infallible>(vec![
        b'd', b'a', b't', b'a', b':', b' ', 0xff, b'\n', b'\n',
    ])]);
    let error = drive_sse(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        source,
        DecodeLimits::production(),
        |_| std::future::ready(()),
    )
    .await
    .unwrap_err_or_else();
    assert!(matches!(error, SseDecodeError::InvalidUtf8));
}

fn session_entry(value: &str) -> SessionEntryId {
    SessionEntryId::new(value).unwrap_or_else(|error| panic!("fixture session entry id: {error}"))
}

fn transcript_item(value: &str) -> TranscriptItemId {
    TranscriptItemId::new(value)
        .unwrap_or_else(|error| panic!("fixture transcript item id: {error}"))
}

fn tool_call(call_id: &str, path: &str) -> ToolCall {
    ToolCall {
        call_id: ToolCallId::new(call_id)
            .unwrap_or_else(|error| panic!("fixture tool call id: {error}")),
        name: "read_file".to_owned(),
        arguments: serde_json::json!({ "path": path }).to_string(),
    }
}

fn opaque_replay_request(compatible_with: ReplayCompatibility) -> ModelRequest {
    let replay = ProviderReplay::new(
        compatible_with,
        r#"{"type":"reasoning","encrypted_content":"ciphertext"}"#.to_owned(),
    )
    .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
    let replay = AssistantReplay::from_positioned([(0, replay)])
        .unwrap_or_else(|error| panic!("fixture assistant replay: {error}"));
    let output = AssistantOutput::new(
        vec![AssistantBlock::Reasoning {
            item_id: transcript_item("private-reasoning"),
            text: String::new(),
        }],
        replay,
    )
    .unwrap_or_else(|error| panic!("fixture assistant output: {error}"));
    ModelRequest {
        session_id: plexmaton_core::SessionId::new("fixture-session")
            .unwrap_or_else(|error| panic!("session: {error}")),
        atoms: vec![
            ContextAtom::assistant(session_entry("private-output"), output)
                .unwrap_or_else(|error| panic!("fixture context atom: {error}")),
        ],
    }
}

fn replay_owner(value: &str) -> ProviderReplayOwnerId {
    ProviderReplayOwnerId::new(value)
        .unwrap_or_else(|error| panic!("fixture replay owner: {error:?}"))
}

fn replay_codec(value: &str) -> ProviderCodecId {
    ProviderCodecId::new(value).unwrap_or_else(|error| panic!("fixture replay codec: {error:?}"))
}

fn replay_model_family(value: &str) -> ProviderModelFamilyId {
    ProviderModelFamilyId::new(value)
        .unwrap_or_else(|error| panic!("fixture model family: {error:?}"))
}
