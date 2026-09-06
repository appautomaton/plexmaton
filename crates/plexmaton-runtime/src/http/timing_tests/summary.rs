use std::future;

use plexmaton_agent::{
    CompactionAttemptFinished, CompactionFailure, CompactionInputMode, CompactionOutcome,
    MAX_COMPACTION_SUMMARY_BYTES, StopReason,
};
use plexmaton_provider::{CompactionInput, encode_request};

use super::*;
use crate::{NativeToolCatalog, http::summary::SummaryCollector};

fn input() -> CompactionInput {
    let mut request = model_call().request;
    request.atoms.push(ContextAtom::user(
        ConversationEntryId::new("summary-instruction").expect("instruction identity"),
        "Summarize the preceding context; return only summary text.".into(),
    ));
    CompactionInput::new(request)
}

async fn summarize_response(api: ModelApi, response: String) -> CompactionAttemptFinished {
    let HttpFixture {
        base_url, server, ..
    } = fixture(Some(response), false);
    let model = model(&base_url, api);
    let catalog = NativeToolCatalog::open(
        std::env::current_dir().expect("fixture workspace"),
        "TEST_KEY",
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .expect("native catalog");
    let tools = catalog.provider_definitions();
    let input = input();
    let expected_body = encode_request(
        &model,
        input.request(),
        &tools,
        Some(model.max_output_tokens()),
    )
    .expect("caller encoding");
    let key = resolve_api_key(&model, Some("fixture-secret".into())).expect("fixture key");
    let http = ProviderHttp::new(
        model,
        key,
        tools,
        Arc::new(FixedWallClock(UnixMillis::EPOCH)),
    )
    .expect("HTTP edge");
    let client = http.summarize(
        attempt(),
        input,
        MAX_COMPACTION_SUMMARY_BYTES,
        CancellationToken::new(),
    );
    let (fact, request) =
        tokio::time::timeout(TEST_TIMEOUT, async { tokio::join!(client, server) })
            .await
            .expect("summary and server settle together");
    let body_start = find_bytes(&request, b"\r\n\r\n").expect("HTTP headers") + 4;
    let actual_body: serde_json::Value =
        serde_json::from_slice(&request[body_start..]).expect("request JSON");
    assert_eq!(
        actual_body, expected_body,
        "summary preserves the caller's complete wire environment"
    );
    assert_eq!(fact.attempt_id(), &attempt());
    assert_eq!(fact.input_mode(), &CompactionInputMode::Verbatim);
    fact
}

/// CPL-2/CPL-6/TIM-3: the real summary route retains tools/settings and collects every dialect.
#[tokio::test]
async fn cpl_6_summary_http_preserves_environment_output_and_accounting_across_dialects() {
    for (api, body, expected_input, expected_output, replay) in [
        (
            ModelApi::OpenaiChatCompletions,
            include_str!("../../../../plexmaton-provider/tests/fixtures/chat_final_answer.sse"),
            20,
            8,
            false,
        ),
        (
            ModelApi::OpenaiResponses,
            include_str!(
                "../../../../plexmaton-provider/tests/fixtures/responses_final_answer.sse"
            ),
            22,
            9,
            true,
        ),
        (
            ModelApi::AnthropicMessages,
            include_str!("../../../../plexmaton-provider/tests/fixtures/messages_final_answer.sse"),
            200,
            7,
            false,
        ),
        (
            ModelApi::GoogleGenerateContent,
            include_str!("../../../../plexmaton-provider/tests/fixtures/gemini_final_answer.sse"),
            200,
            9,
            true,
        ),
    ] {
        let fact = summarize_response(api, response("200 OK", body)).await;
        let CompactionOutcome::Complete { output } = fact.outcome() else {
            panic!("{api:?}: successful summary: {:?}", fact.outcome());
        };
        let text: String = output
            .blocks()
            .iter()
            .filter_map(|block| match block {
                AssistantBlock::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Plexmaton.");
        assert_eq!(output.replay().is_some(), replay, "{api:?} replay");
        if matches!(
            api,
            ModelApi::OpenaiChatCompletions | ModelApi::OpenaiResponses
        ) {
            assert!(output.blocks().iter().any(|block| matches!(
                block, AssistantBlock::Reasoning { text, .. } if text.trim() == "The file names the project."
            )));
        }
        let RequestAttemptTerminalState::Dispatched {
            timing,
            outcome,
            usage,
            cost,
        } = fact.terminal().terminal()
        else {
            panic!("summary dispatched");
        };
        assert_eq!(
            outcome,
            &RequestDispatchedOutcome::Completed {
                stop_reason: StopReason::EndOfTurn
            }
        );
        let counts = usage.counts().expect("summary usage");
        assert_eq!(
            (counts.input, counts.output),
            (expected_input, expected_output)
        );
        assert!(matches!(cost, RequestCost::Known { .. }));
        assert!(timing.headers_after_ms().is_some());
        assert!(timing.first_output_after_ms().is_some());
    }
}

/// CPL-6: tool-producing summaries end as audited failures through all four real decoders.
#[tokio::test]
async fn cpl_6_summary_http_rejects_tools_and_keeps_their_output_for_audit() {
    for (api, body) in [
        (
            ModelApi::OpenaiChatCompletions,
            include_str!("../../../../plexmaton-provider/tests/fixtures/chat_tool_call.sse"),
        ),
        (
            ModelApi::OpenaiResponses,
            include_str!("../../../../plexmaton-provider/tests/fixtures/responses_tool_call.sse"),
        ),
        (
            ModelApi::AnthropicMessages,
            include_str!("../../../../plexmaton-provider/tests/fixtures/messages_tool_call.sse"),
        ),
        (
            ModelApi::GoogleGenerateContent,
            include_str!("../../../../plexmaton-provider/tests/fixtures/gemini_tool_call.sse"),
        ),
    ] {
        let fact = summarize_response(api, response("200 OK", body)).await;
        assert_eq!(
            fact.outcome().failure(),
            Some(CompactionFailure::ToolCallOutput),
            "{api:?}"
        );
        let output = fact.outcome().output().expect("collected tool output");
        assert_eq!(output.tool_calls().count(), 1, "{api:?}");
        assert_eq!(
            output.tool_calls().next().expect("tool call").name,
            "read_file"
        );
        assert!(matches!(
            fact.terminal().terminal(),
            RequestAttemptTerminalState::Dispatched {
                outcome: RequestDispatchedOutcome::Completed {
                    stop_reason: StopReason::ToolCalls
                },
                usage: TokenUsage::Complete(_),
                ..
            }
        ));
    }
}

/// CPL-6/CPL-8: typed HTTP and stream failures keep honest raw terminals and received usage.
#[tokio::test]
async fn cpl_6_summary_http_failures_keep_raw_terminal_and_partial_output() {
    for (status, body, expected, raw) in [
        (
            "400 Bad Request",
            r#"{"error":{"code":"context_length_exceeded"}}"#,
            CompactionFailure::ContextTooLong,
            RequestDispatchedOutcome::ContextTooLong,
        ),
        (
            "429 Too Many Requests",
            r#"{"error":{"message":"limited"}}"#,
            CompactionFailure::ProviderFailed,
            RequestDispatchedOutcome::RateLimited,
        ),
        (
            "503 Service Unavailable",
            r#"{"error":{"message":"unavailable"}}"#,
            CompactionFailure::ProviderFailed,
            RequestDispatchedOutcome::ProviderFailed,
        ),
    ] {
        let fact =
            summarize_response(ModelApi::OpenaiChatCompletions, response(status, body)).await;
        assert_eq!(fact.outcome().failure(), Some(expected));
        assert!(
            matches!(fact.terminal().terminal(), RequestAttemptTerminalState::Dispatched { outcome, usage: TokenUsage::Unavailable, .. } if outcome == &raw)
        );
    }
    for (suffix, expected, raw) in [
        (
            "data: invalid-json\n\n".to_owned(),
            CompactionFailure::Malformed,
            RequestDispatchedOutcome::Malformed,
        ),
        (
            chat_chunk(serde_json::json!({}), Some("length"), None) + "data: [DONE]\n\n",
            CompactionFailure::OutputLimit,
            RequestDispatchedOutcome::Completed {
                stop_reason: StopReason::OutputLimit,
            },
        ),
    ] {
        let body = chat_chunk(
            serde_json::json!({"content":"partial"}),
            None,
            Some(usage_json()),
        ) + &suffix;
        let fact =
            summarize_response(ModelApi::OpenaiChatCompletions, response("200 OK", &body)).await;
        assert_eq!(fact.outcome().failure(), Some(expected));
        assert!(
            matches!(&fact.outcome().output().expect("partial audit").blocks()[0], AssistantBlock::Text { text, .. } if text == "partial")
        );
        assert!(
            matches!(fact.terminal().terminal(), RequestAttemptTerminalState::Dispatched { outcome, usage, .. } if outcome == &raw && usage == &complete_usage())
        );
    }
}

/// CPL-6/CPL-7: cancellation before dispatch has no fabricated timing, usage or output.
#[tokio::test]
async fn cpl_7_summary_http_cancels_before_dispatch() {
    let http = http(
        "http://127.0.0.1:1/v1",
        ModelApi::OpenaiChatCompletions,
        UnixMillis::EPOCH,
    );
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let fact = http.summarize(attempt(), input(), 1024, cancellation).await;
    assert_eq!(fact.outcome().failure(), Some(CompactionFailure::Cancelled));
    assert!(fact.outcome().output().is_none());
    assert!(matches!(
        fact.terminal().terminal(),
        RequestAttemptTerminalState::NotDispatched {
            outcome: RequestNotDispatchedOutcome::Cancelled
        }
    ));
}

/// CPL-6/CPL-7/TIM-3: cancellation after decoded output preserves observed usage and joins HTTP.
#[tokio::test]
async fn cpl_7_summary_http_cancellation_keeps_observed_output_and_usage() {
    let body = chat_chunk(serde_json::json!({}), None, Some(usage_json()))
        + &chat_chunk(serde_json::json!({"content":"observed"}), None, None);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n",
        body.len()
    );
    let HttpFixture {
        base_url,
        release,
        server,
        ..
    } = fixture(Some(response), true);
    let http = http(
        &base_url,
        ModelApi::OpenaiChatCompletions,
        UnixMillis::EPOCH,
    );
    let cancellation = CancellationToken::new();
    let mut collector = SummaryCollector::new(attempt(), 1024);
    let (marker, observed) = oneshot::channel();
    let mut marker = Some(marker);
    // The production transport/collector seam supplies readiness after decoding, without a fake step.
    let client = async {
        let report = {
            let future = http.perform_events(
                attempt(),
                input().into_request(),
                cancellation.child_token(),
                |output| {
                    let event = output.into_event();
                    let text = matches!(event, ModelEvent::TextDelta { .. });
                    collector.push(event);
                    if text && let Some(marker) = marker.take() {
                        marker.send(()).expect("observer alive");
                    }
                    future::ready(())
                },
            );
            tokio::pin!(future);
            tokio::select! {
                report = &mut future => panic!("summary ended before cancel: {:?}", report.completion),
                result = observed => result.expect("decoded text after usage"),
            }
            cancellation.cancel();
            future.await
        };
        release.send(()).expect("release server");
        collector.finish(report)
    };
    let (fact, _) = tokio::time::timeout(TEST_TIMEOUT, async { tokio::join!(client, server) })
        .await
        .expect("both owners settle");
    assert_eq!(fact.outcome().failure(), Some(CompactionFailure::Cancelled));
    assert!(
        matches!(&fact.outcome().output().expect("partial audit").blocks()[0], AssistantBlock::Text { text, .. } if text == "observed")
    );
    assert!(
        matches!(fact.terminal().terminal(), RequestAttemptTerminalState::Dispatched {
        outcome: RequestDispatchedOutcome::Cancelled, usage, cost: RequestCost::Unavailable, ..
    } if usage == &complete_usage())
    );
}
