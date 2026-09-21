use super::*;
use std::convert::Infallible;

use futures_util::stream;
use plexmaton_core::{ServerToolAction, ServerToolStatus};
use plexmaton_provider::{DecodeError, DecodeLimits, SseDecodeError, drive_sse};

const CREATED: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_fixture_ws","status":"in_progress","output":[],"store":false}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"id":"ws_fixture_x","type":"web_search_call","status":"in_progress"}}

"#;
const COMPLETED: &str = r#"event: response.completed
data: {"type":"response.completed","sequence_number":3,"response":{"id":"resp_fixture_ws","status":"completed","usage":{"input_tokens":10,"input_tokens_details":{"cached_tokens":0},"output_tokens":2,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":12}}}

"#;

fn search_done(action: &str) -> String {
    done_with_status("completed", action)
}

fn done_with_status(status: &str, action: &str) -> String {
    format!(
        "{CREATED}event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"sequence_number\":2,\"output_index\":0,\"item\":{{\"id\":\"ws_fixture_x\",\"type\":\"web_search_call\",\"status\":\"{status}\"{action}}}}}\n\n{COMPLETED}"
    )
}

/// PRV-5: a call the provider gave up on is carried with that outcome, so the row can say so, and
/// a done item that still claims to be running fails the step rather than being read as either.
#[tokio::test]
async fn prv_5_a_server_tool_calls_outcome_is_typed_from_its_status() {
    for (status, expected) in [
        ("failed", ServerToolStatus::Failed),
        ("incomplete", ServerToolStatus::Failed),
        ("completed", ServerToolStatus::Completed),
    ] {
        let (result, emitted) = decode(&done_with_status(
            status,
            r#","action":{"type":"search","query":"rust release"}"#,
        ))
        .await;
        assert!(result.is_ok(), "{status}: {result:?}");
        assert!(
            matches!(
                emitted
                .iter()
                .find(|event| matches!(event, ModelEvent::ServerToolCall { .. })),
                Some(ModelEvent::ServerToolCall { call, .. }) if call.status == expected
            ),
            "{status}: {emitted:?}"
        );
    }
    for status in ["in_progress", "searching"] {
        let (result, emitted) = decode(&done_with_status(
            status,
            r#","action":{"type":"search","query":"rust release"}"#,
        ))
        .await;
        assert!(
            matches!(
                result,
                Err(SseDecodeError::Decode(DecodeError::UnsupportedEvent(ref kind)))
                    if *kind == format!("web_search_call.status:{status}")
            ),
            "{status}: {result:?}"
        );
        assert!(
            emitted
                .iter()
                .all(|event| matches!(event, ModelEvent::ServerToolStarted { .. })),
            "{status}: {emitted:?}"
        );
    }
}

async fn decode(source: &str) -> (Result<(), SseDecodeError<Infallible>>, Vec<ModelEvent>) {
    let profile = profile(ModelApi::OpenaiResponses);
    let mut emitted = Vec::new();
    let result = drive_sse(
        &plexmaton_agent::RequestAttemptId::new("fixture-attempt")
            .unwrap_or_else(|error| panic!("attempt: {error}")),
        &profile,
        stream::iter([Ok::<_, Infallible>(source.as_bytes())]),
        DecodeLimits::production(),
        |event| {
            emitted.push(event);
            std::future::ready(())
        },
    )
    .await;
    (result, emitted)
}

/// PRV-5: an action kind the record cannot name fails the step rather than being carried blind.
#[tokio::test]
async fn prv_5_an_unknown_server_tool_action_fails_the_step() {
    let (result, emitted) = decode(&search_done(
        r#","action":{"type":"teleport","destination":"mars"}"#,
    ))
    .await;
    assert!(
        matches!(
            result,
            Err(SseDecodeError::Decode(DecodeError::UnsupportedEvent(ref kind)))
                if kind == "web_search_call.action:teleport"
        ),
        "{result:?}"
    );
    assert!(
        emitted
            .iter()
            .all(|event| matches!(event, ModelEvent::ServerToolStarted { .. })),
        "nothing beyond the call's placement: {emitted:?}"
    );
}

/// PRV-5: a finished call without an action says nothing about what the provider did.
#[tokio::test]
async fn prv_5_a_server_tool_call_without_an_action_fails_the_step() {
    let (result, _) = decode(&search_done("")).await;
    assert!(
        matches!(
            result,
            Err(SseDecodeError::Decode(DecodeError::UnsupportedEvent(ref kind)))
                if kind == "web_search_call_without_action"
        ),
        "{result:?}"
    );
}

/// PRV-5: a search that arrived with no query is carried as it arrived, and a query spelled twice
/// is kept once. Neither refuses the step.
#[tokio::test]
async fn prv_5_a_search_without_a_query_is_carried_and_a_repeated_query_is_kept_once() {
    let (result, emitted) = decode(&search_done(r#","action":{"type":"search","query":""}"#)).await;
    result.unwrap_or_else(|error| panic!("empty query should decode: {error}"));
    assert!(
        matches!(
            emitted.as_slice(),
            [ModelEvent::ServerToolStarted { .. }, ModelEvent::ServerToolCall { call, .. }, ModelEvent::Replay { .. }, ModelEvent::Usage(_), ModelEvent::Stopped(StopReason::EndOfTurn)]
                if call.action == ServerToolAction::Search { queries: Vec::new() }
        ),
        "{emitted:?}"
    );
    let (result, emitted) = decode(&search_done(
        r#","action":{"type":"search","query":"rust 1.98.1","queries":["rust 1.98.1","rust release"]}"#,
    ))
    .await;
    result.unwrap_or_else(|error| panic!("two spellings should decode: {error}"));
    assert!(
        matches!(
            emitted
                .iter()
                .find(|event| matches!(event, ModelEvent::ServerToolCall { .. })),
            Some(ModelEvent::ServerToolCall { call, .. })
                if call.action == ServerToolAction::Search {
                    queries: vec!["rust 1.98.1".to_owned(), "rust release".to_owned()]
                }
        ),
        "{emitted:?}"
    );
}
