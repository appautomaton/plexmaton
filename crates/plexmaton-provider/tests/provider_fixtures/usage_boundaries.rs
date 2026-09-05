use plexmaton_agent::{ModelEvent, StopReason};
use plexmaton_core::TokenUsage;
use plexmaton_provider::{DecodeError, DecodeLimits, ModelApi, ProviderCodec};

use super::support::profile;

/// LIVE-4: even a direct codec caller receives usage before the stop that closes its model step.
#[test]
fn a_combined_chat_terminal_chunk_orders_usage_before_stop() {
    let profile = profile(ModelApi::OpenaiChatCompletions);
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        DecodeLimits::production(),
    );
    let combined = r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":2,"total_tokens":10}}"#;

    assert!(matches!(
        codec.push_sse("message", combined),
        Ok(events)
            if matches!(events.as_slice(), [
                ModelEvent::Usage(TokenUsage::Partial(counts)),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ] if counts.total == 10)
    ));
}

/// LIVE-5: optional `null` usage breakdowns mean the provider omitted them, not that the stream
/// carrying otherwise exact top-level counts was malformed.
#[test]
fn responses_null_usage_breakdowns_are_partial_coverage() {
    let profile = profile(ModelApi::OpenaiResponses);
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        DecodeLimits::production(),
    );
    let completed = r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":8,"input_tokens_details":{"cached_tokens":null,"cache_write_tokens":null},"output_tokens":2,"output_tokens_details":{"reasoning_tokens":null},"total_tokens":10}}}"#;

    assert!(matches!(
        codec.push_sse("response.completed", completed),
        Ok(events)
            if matches!(events.as_slice(), [
                ModelEvent::Usage(TokenUsage::Partial(counts)),
                ModelEvent::Stopped(StopReason::EndOfTurn),
            ] if counts.input == 8
                && counts.output == 2
                && counts.total == 10
                && counts.cached_input.is_none()
                && counts.cache_write_input.is_none()
                && counts.reasoning_output.is_none())
    ));
}

/// TIM-3: cached and cache-written tokens are disjoint input subsets, never two overlapping bills.
#[test]
fn combined_input_breakdowns_cannot_exceed_provider_input() {
    let profile = profile(ModelApi::OpenaiChatCompletions);
    let mut codec = ProviderCodec::new(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        DecodeLimits::production(),
    );
    let invalid = r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":2,"total_tokens":10,"prompt_tokens_details":{"cached_tokens":6,"cache_write_tokens":4},"completion_tokens_details":{"reasoning_tokens":0}}}"#;

    assert!(matches!(
        codec.push_sse("message", invalid),
        Err(DecodeError::InvalidUsage {
            field: "input_breakdown"
        })
    ));
}
