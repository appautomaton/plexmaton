use std::{net::TcpListener, sync::Arc, time::Duration};

use futures_util::{FutureExt as _, future::BoxFuture};

use plexmaton_agent::{
    Agent, AssistantBlock, AssistantOutput, AssistantReplay, ContextAtom, Effect, Input, ModelCall,
    ModelError, ModelEvent, ModelRequest, ProviderReplay, RequestAttemptId,
    RequestAttemptTerminalState, RequestCost, RequestDispatchedOutcome,
    RequestNotDispatchedOutcome, UnixMillis, UsdCostTicks,
};
use plexmaton_core::{AgentId, SessionEntryId, TokenCounts, TokenUsage, TranscriptItemId};
use plexmaton_provider::{
    FunctionTool, ModelApi, ModelRegistry, ResolvedModel, request_environment, resolve_api_key,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    sync::{mpsc, oneshot},
};
use tokio_util::sync::CancellationToken;

use super::OpenAiHttp;
use crate::runtime::{FixedWallClock, ModelCompletion, ModelDriver, ModelTerminalReport};

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

const FINAL_ANSWER: &str =
    include_str!("../../../plexmaton-provider/tests/fixtures/responses_final_answer.sse");

fn model(base_url: &str, api: ModelApi) -> ResolvedModel {
    let api = match api {
        ModelApi::OpenaiResponses => "openai_responses",
        ModelApi::OpenaiChatCompletions => "openai_chat_completions",
    };
    ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "test", model = "luna" }}
[providers.test]
base_url = "{base_url}"
api_key_env = "TEST_KEY"
[providers.test.models.luna]
api = "{api}"
id = "gpt-5.6-luna"
reasoning_effort = "low"
context_window_tokens = 272000
max_output_tokens = 128000
output_reserve_tokens = 16384
cost = {{ input = 1.0, output = 2.0, cache_read = 0.1, cache_write = 1.5 }}
"#
    ))
    .unwrap_or_else(|error| panic!("fixture profile: {error}"))
    .active_model()
    .clone()
}

fn http(base_url: &str, api: ModelApi, wall: UnixMillis) -> OpenAiHttp {
    let model = model(base_url, api);
    let key = resolve_api_key(&model, Some("fixture-secret".into()))
        .unwrap_or_else(|error| panic!("fixture key: {error}"));
    OpenAiHttp::new(
        model,
        key,
        Arc::<[FunctionTool]>::from([]),
        Arc::new(FixedWallClock(wall)),
    )
    .unwrap_or_else(|error| panic!("fixture HTTP edge: {error}"))
}

fn attempt() -> RequestAttemptId {
    RequestAttemptId::new("attempt-http-1")
        .unwrap_or_else(|error| panic!("fixture attempt: {error}"))
}

fn model_call() -> ModelCall {
    let agent_id =
        AgentId::new("http-fixture").unwrap_or_else(|error| panic!("fixture agent id: {error}"));
    let mut agent = Agent::new(agent_id);
    agent
        .handle_at(
            Input::Submitted {
                text: "hello".to_owned(),
            },
            UnixMillis::EPOCH,
        )
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::CallModel(call) => Some(call),
            Effect::AdmitTool(_) | Effect::RunTool(_) => None,
        })
        .unwrap_or_else(|| panic!("submission opens one model call"))
}

fn opaque_chat_call(model: &ResolvedModel) -> ModelCall {
    let mut call = model_call();
    let replay = ProviderReplay::new(model.replay_compatibility(), "ciphertext".to_owned())
        .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
    let replay = AssistantReplay::from_positioned([(0, replay)])
        .unwrap_or_else(|error| panic!("fixture assistant replay: {error}"));
    let output = AssistantOutput::new(
        vec![AssistantBlock::Reasoning {
            item_id: TranscriptItemId::new("reasoning-http-fixture")
                .unwrap_or_else(|error| panic!("fixture item: {error}")),
            text: String::new(),
        }],
        replay,
    )
    .unwrap_or_else(|error| panic!("fixture assistant output: {error}"));
    call.request = ModelRequest {
        atoms: vec![
            ContextAtom::assistant(
                SessionEntryId::new("entry-http-fixture")
                    .unwrap_or_else(|error| panic!("fixture entry: {error}")),
                output,
            )
            .unwrap_or_else(|error| panic!("fixture context atom: {error}")),
        ],
    };
    call
}

/// TIM-2/TIM-5: cancellation and encoding failures before `.send()` retain no fabricated timing.
#[tokio::test]
async fn pre_dispatch_outcomes_have_no_request_measurements_or_signals() {
    let cancelled_http = http(
        "http://127.0.0.1:1/v1",
        ModelApi::OpenaiResponses,
        UnixMillis::new(700),
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let (signals, mut received) = mpsc::channel(8);
    let report = cancelled_http
        .drive(attempt(), model_call(), signals, cancelled)
        .await;
    assert!(matches!(
        report.terminal.terminal(),
        RequestAttemptTerminalState::NotDispatched {
            outcome: RequestNotDispatchedOutcome::Cancelled
        }
    ));
    assert!(matches!(report.completion, ModelCompletion::Cancelled));
    assert!(received.try_recv().is_err());

    let chat = model("http://127.0.0.1:1/v1", ModelApi::OpenaiChatCompletions);
    let expected_environment = request_environment(&chat, &[], Some(chat.max_output_tokens()));
    let key = resolve_api_key(&chat, Some("fixture-secret".into()))
        .unwrap_or_else(|error| panic!("fixture key: {error}"));
    let encoding_http = OpenAiHttp::new(
        chat.clone(),
        key,
        Arc::<[FunctionTool]>::from([]),
        Arc::new(FixedWallClock(UnixMillis::new(701))),
    )
    .unwrap_or_else(|error| panic!("fixture HTTP edge: {error}"));
    assert_eq!(encoding_http.request_environment(), &expected_environment);
    let (signals, mut received) = mpsc::channel(8);
    let report = encoding_http
        .drive(
            attempt(),
            opaque_chat_call(&chat),
            signals,
            CancellationToken::new(),
        )
        .await;
    assert!(matches!(
        report.terminal.terminal(),
        RequestAttemptTerminalState::NotDispatched {
            outcome: RequestNotDispatchedOutcome::EncodingFailed
        }
    ));
    assert!(matches!(
        report.completion,
        ModelCompletion::Failed(ModelError::Malformed { .. })
    ));
    assert!(received.try_recv().is_err());
}

/// TIM-2/TIM-3: one dispatched request owns ordered milestones and terminal-only exact usage.
#[tokio::test]
async fn dispatched_response_returns_one_correlated_terminal_report() {
    let fixed_wall = UnixMillis::new(1_788_000_000_123);
    let step_id = model_call().step_id;
    let attempt_id = attempt();
    let (report, outputs, request) = exchange(
        Some(response("200 OK", FINAL_ANSWER)),
        ModelApi::OpenaiResponses,
        fixed_wall,
    )
    .await;
    assert!(String::from_utf8_lossy(&request).contains("POST /v1/responses"));
    assert_eq!(report.step_id, step_id);
    assert_eq!(report.terminal.attempt_id(), &attempt_id);
    let RequestAttemptTerminalState::Dispatched {
        timing,
        outcome,
        usage,
        cost,
    } = report.terminal.terminal()
    else {
        panic!("successful fixture request was dispatched");
    };
    assert_eq!(timing.dispatched_at(), fixed_wall);
    let headers = timing
        .headers_after_ms()
        .unwrap_or_else(|| panic!("response headers milestone"));
    let first = timing
        .first_output_after_ms()
        .unwrap_or_else(|| panic!("first semantic output milestone"));
    assert!(headers <= first);
    assert!(first <= timing.terminal_after_ms());
    assert!(matches!(
        outcome,
        RequestDispatchedOutcome::Completed { .. }
    ));
    assert!(matches!(
        usage,
        TokenUsage::Complete(counts) if counts.total == 31
    ));
    assert_eq!(
        cost,
        &RequestCost::Known {
            usd_ticks: UsdCostTicks::new(347_000)
        }
    );
    assert!(matches!(report.completion, ModelCompletion::Stopped(_)));

    assert!(!outputs.is_empty());
    assert!(
        outputs
            .iter()
            .all(|event| !matches!(event, ModelEvent::Usage(_) | ModelEvent::Stopped(_)))
    );
}

/// TIM-2/TIM-5: cancellation after the server observes a request stays dispatched even without
/// response headers.
#[tokio::test]
async fn cancellation_after_dispatch_keeps_only_observed_milestones() {
    let HttpFixture {
        base_url,
        observed,
        release,
        server,
    } = fixture(None, true);
    let fixed_wall = UnixMillis::new(1_788_000_000_456);
    let http = http(&base_url, ModelApi::OpenaiResponses, fixed_wall);
    let cancellation = CancellationToken::new();
    let (signals, mut received) = mpsc::channel(8);
    let future = http.drive(attempt(), model_call(), signals, cancellation.child_token());
    let client = async {
        tokio::pin!(future);
        tokio::select! {
            report = &mut future => panic!("request ended before cancellation: {report:?}"),
            observed = observed => observed.expect("server observed dispatch"),
        }
        cancellation.cancel();
        let report = future.await;
        release.send(()).expect("release fixture server");
        report
    };
    let (report, _) = tokio::time::timeout(TEST_TIMEOUT, async { tokio::join!(client, server) })
        .await
        .expect("pre-headers cancellation settles both owners");

    let RequestAttemptTerminalState::Dispatched {
        timing,
        outcome,
        usage,
        cost,
    } = report.terminal.terminal()
    else {
        panic!("observed request crossed dispatch boundary");
    };
    assert_eq!(timing.dispatched_at(), fixed_wall);
    assert_eq!(timing.headers_after_ms(), None);
    assert_eq!(timing.first_output_after_ms(), None);
    assert!(matches!(outcome, RequestDispatchedOutcome::Cancelled));
    assert_eq!(usage, &TokenUsage::Unavailable);
    assert_eq!(cost, &RequestCost::Unavailable);
    assert!(matches!(report.completion, ModelCompletion::Cancelled));
    assert!(received.try_recv().is_err());
}

/// TIM-2/TIM-5: every HTTP failure retains the dispatch boundary and only observed milestones.
#[tokio::test]
async fn dispatched_http_failures_preserve_their_terminal_measurements() {
    let text = chat_chunk(serde_json::json!({"content": "partial"}), None, None);
    let cases = [
        (
            None,
            RequestDispatchedOutcome::TransportFailed,
            false,
            false,
        ),
        (
            Some(response("429 Too Many Requests", "{}")),
            RequestDispatchedOutcome::RateLimited,
            true,
            false,
        ),
        (
            Some(response(
                "400 Bad Request",
                r#"{"error":{"code":"context_length_exceeded"}}"#,
            )),
            RequestDispatchedOutcome::ContextTooLong,
            true,
            false,
        ),
        (
            Some(response("200 OK", "data: malformed-json\n\n")),
            RequestDispatchedOutcome::Malformed,
            true,
            false,
        ),
        (
            Some(response("200 OK", &text)),
            RequestDispatchedOutcome::Malformed,
            true,
            true,
        ),
    ];
    for (response, expected, has_headers, has_output) in cases {
        let wall = UnixMillis::new(800);
        let (report, output, _) = exchange(response, ModelApi::OpenaiChatCompletions, wall).await;
        let RequestAttemptTerminalState::Dispatched {
            timing,
            outcome,
            usage,
            cost,
        } = report.terminal.terminal()
        else {
            panic!("HTTP failure must retain dispatch: {expected:?}");
        };
        assert_eq!(outcome, &expected);
        assert_eq!(timing.dispatched_at(), wall);
        assert_eq!(timing.headers_after_ms().is_some(), has_headers);
        assert_eq!(timing.first_output_after_ms().is_some(), has_output);
        assert_eq!(!output.is_empty(), has_output);
        assert_eq!(usage, &TokenUsage::Unavailable);
        assert_eq!(cost, &RequestCost::Unavailable);
        assert!(matches!(
            (&expected, &report.completion),
            (
                RequestDispatchedOutcome::TransportFailed,
                ModelCompletion::Failed(ModelError::Transport { .. })
            ) | (
                RequestDispatchedOutcome::RateLimited,
                ModelCompletion::Failed(ModelError::RateLimited { .. })
            ) | (
                RequestDispatchedOutcome::ContextTooLong,
                ModelCompletion::Failed(ModelError::ContextTooLong)
            ) | (
                RequestDispatchedOutcome::Malformed,
                ModelCompletion::Failed(ModelError::Malformed { .. })
            )
        ));
    }
}

/// TIM-2/TIM-3/TIM-5: outputless accounting cannot fabricate first output; missing usage stays
/// unavailable, while complete tools and opaque replay each establish their own output milestone.
#[tokio::test]
async fn first_output_distinguishes_semantic_content_from_usage_and_stop() {
    let stop = chat_chunk(serde_json::json!({}), Some("stop"), None);
    let accounting = chat_chunk(serde_json::json!({}), Some("stop"), Some(usage_json()));
    let tool = chat_chunk(
        serde_json::json!({"tool_calls": [{
            "index": 0, "id": "call_timing", "type": "function",
            "function": {"name": "read_file", "arguments": "{}"}
        }]}),
        Some("tool_calls"),
        None,
    );
    let replay = concat!(
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"summary\":[],\"encrypted_content\":\"fixture-ciphertext\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    );
    let cases = [
        (
            ModelApi::OpenaiChatCompletions,
            format!("{stop}data: [DONE]\n\n"),
            false,
            false,
        ),
        (
            ModelApi::OpenaiChatCompletions,
            format!("{accounting}data: [DONE]\n\n"),
            false,
            true,
        ),
        (
            ModelApi::OpenaiChatCompletions,
            format!("{tool}data: [DONE]\n\n"),
            true,
            false,
        ),
        (ModelApi::OpenaiResponses, replay.to_owned(), true, false),
    ];
    for (api, body, has_output, has_usage) in cases {
        let (report, output, _) =
            exchange(Some(response("200 OK", &body)), api, UnixMillis::new(900)).await;
        let RequestAttemptTerminalState::Dispatched {
            timing,
            outcome,
            usage,
            cost,
        } = report.terminal.terminal()
        else {
            panic!("successful fixture crossed dispatch");
        };
        assert!(matches!(
            outcome,
            RequestDispatchedOutcome::Completed { .. }
        ));
        assert!(timing.headers_after_ms().is_some());
        assert_eq!(timing.first_output_after_ms().is_some(), has_output);
        assert_eq!(!output.is_empty(), has_output);
        assert!(
            output.iter().all(|event| matches!(
                event,
                ModelEvent::Called { .. } | ModelEvent::Replay { .. }
            ))
        );
        let expected_usage = if has_usage {
            complete_usage()
        } else {
            TokenUsage::Unavailable
        };
        let expected_cost = if has_usage {
            complete_cost()
        } else {
            RequestCost::Unavailable
        };
        assert_eq!(usage, &expected_usage);
        assert_eq!(cost, &expected_cost);
    }
}

/// TIM-3/TIM-5: a failure after accounting retains the exact observed usage and frozen price.
#[tokio::test]
async fn malformed_stream_after_usage_preserves_reported_consumption() {
    let accounting = chat_chunk(serde_json::json!({}), Some("stop"), Some(usage_json()));
    let body = format!("{accounting}data: malformed-trailer\n\n");
    let (report, output, _) = exchange(
        Some(response("200 OK", &body)),
        ModelApi::OpenaiChatCompletions,
        UnixMillis::new(950),
    )
    .await;
    assert!(matches!(
        report.terminal.terminal(),
        RequestAttemptTerminalState::Dispatched {
            outcome: RequestDispatchedOutcome::Malformed,
            usage,
            cost,
            ..
        } if usage == &complete_usage() && cost == &complete_cost()
    ));
    assert!(output.is_empty());
}

/// TIM-2/TIM-3/TIM-5: cancellation after a visible marker retains prior usage but never invents it
/// when none arrived. The marker follows accounting on the wire, proving the decoder consumed it.
#[tokio::test]
async fn cancellation_after_output_preserves_only_observed_usage() {
    for with_usage in [false, true] {
        let mut body = String::new();
        if with_usage {
            body.push_str(&chat_chunk(serde_json::json!({}), None, Some(usage_json())));
        }
        body.push_str(&chat_chunk(
            serde_json::json!({"content": "observed"}),
            None,
            None,
        ));
        let partial_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n",
            body.len()
        );
        let HttpFixture {
            base_url,
            release,
            server,
            ..
        } = fixture(Some(partial_response), true);
        let http = http(
            &base_url,
            ModelApi::OpenaiChatCompletions,
            UnixMillis::new(1_000),
        );
        let cancellation = CancellationToken::new();
        let (signals, mut received) = mpsc::channel(8);
        let future = http.drive(attempt(), model_call(), signals, cancellation.child_token());
        let client = async {
            tokio::pin!(future);
            let signal = tokio::select! {
                report = &mut future => panic!("held stream ended before cancellation: {report:?}"),
                signal = received.recv() => signal.expect("visible marker follows prior usage"),
            };
            assert!(
                matches!(signal.output.into_event(), ModelEvent::TextDelta { delta, .. } if delta == "observed")
            );
            cancellation.cancel();
            let report = future.await;
            release.send(()).expect("release cancelled stream");
            report
        };
        let (report, _) =
            tokio::time::timeout(TEST_TIMEOUT, async { tokio::join!(client, server) })
                .await
                .expect("output cancellation settles both owners");
        assert_eq!(report.terminal.attempt_id(), &attempt());
        assert_eq!(report.step_id, model_call().step_id);
        let RequestAttemptTerminalState::Dispatched {
            timing,
            outcome,
            usage,
            cost,
        } = report.terminal.terminal()
        else {
            panic!("cancelled streamed request was dispatched");
        };
        assert_eq!(outcome, &RequestDispatchedOutcome::Cancelled);
        assert!(matches!(report.completion, ModelCompletion::Cancelled));
        assert_eq!(timing.dispatched_at(), UnixMillis::new(1_000));
        let headers = timing.headers_after_ms().expect("headers observed");
        let first = timing.first_output_after_ms().expect("text observed");
        assert!(headers <= first && first <= timing.terminal_after_ms());
        let expected_usage = if with_usage {
            complete_usage()
        } else {
            TokenUsage::Unavailable
        };
        let expected_cost = if with_usage {
            complete_cost()
        } else {
            RequestCost::Unavailable
        };
        assert_eq!(usage, &expected_usage);
        assert_eq!(cost, &expected_cost);
        assert!(received.try_recv().is_err(), "accounting is terminal-only");
    }
}

fn chat_chunk(
    delta: serde_json::Value,
    stop: Option<&str>,
    usage: Option<serde_json::Value>,
) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({"choices": [{"index": 0, "delta": delta, "finish_reason": stop}], "usage": usage})
    )
}

fn usage_json() -> serde_json::Value {
    serde_json::json!({
        "prompt_tokens": 10, "prompt_tokens_details": {"cached_tokens": 2, "cache_write_tokens": 1},
        "completion_tokens": 4, "completion_tokens_details": {"reasoning_tokens": 1},
        "total_tokens": 14
    })
}

fn complete_usage() -> TokenUsage {
    TokenUsage::Complete(TokenCounts {
        input: 10,
        cached_input: Some(2),
        cache_write_input: Some(1),
        output: 4,
        reasoning_output: Some(1),
        total: 14,
    })
}

fn complete_cost() -> RequestCost {
    RequestCost::Known {
        usd_ticks: UsdCostTicks::new(167_000),
    }
}

struct HttpFixture {
    base_url: String,
    observed: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
    server: BoxFuture<'static, Vec<u8>>,
}

/// The test jointly polls both owners under one timeout; dropping it closes all sockets.
fn fixture(response: Option<String>, hold_open: bool) -> HttpFixture {
    let listener = TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("bind fixture server: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("fixture address: {error}"));
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let listener = tokio::net::TcpListener::from_std(listener).expect("async fixture listener");
    let (observed_tx, observed) = oneshot::channel();
    let (release, release_rx) = oneshot::channel();
    let server = async move {
        let (mut stream, _) = listener.accept().await.expect("accept fixture request");
        let request = read_request(&mut stream).await;
        let _observer_closed = observed_tx.send(());
        if let Some(response) = response {
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write fixture response");
        }
        if hold_open {
            release_rx.await.expect("client releases held response");
        }
        request
    }
    .boxed();
    HttpFixture {
        base_url: format!("http://{address}/v1"),
        observed,
        release,
        server,
    }
}

fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn exchange(
    response: Option<String>,
    api: ModelApi,
    wall: UnixMillis,
) -> (ModelTerminalReport, Vec<ModelEvent>, Vec<u8>) {
    let HttpFixture {
        base_url, server, ..
    } = fixture(response, false);
    let http = http(&base_url, api, wall);
    let call = model_call();
    let step_id = call.step_id.clone();
    let (signals, mut received) = mpsc::channel(16);
    let client = http.drive(attempt(), call, signals, CancellationToken::new());
    let (report, request) =
        tokio::time::timeout(TEST_TIMEOUT, async { tokio::join!(client, server) })
            .await
            .expect("HTTP fixture settles both owners");
    assert_eq!(report.step_id, step_id);
    assert_eq!(report.terminal.attempt_id(), &attempt());
    let mut output = Vec::new();
    while let Ok(signal) = received.try_recv() {
        assert_eq!(signal.attempt_id, attempt());
        assert_eq!(signal.step_id, step_id);
        output.push(signal.output.into_event());
    }
    (report, output, request)
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let count = stream
            .read(&mut buffer)
            .await
            .expect("read fixture request");
        assert_ne!(count, 0, "request ended before its body");
        request.extend_from_slice(&buffer[..count]);
        assert!(request.len() <= 64 * 1024, "fixture request exceeded bound");
        let Some(header_end) = find_bytes(&request, b"\r\n\r\n") else {
            continue;
        };
        let body_start = header_end + 4;
        let headers =
            std::str::from_utf8(&request[..header_end]).expect("fixture request headers are UTF-8");
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .expect("fixture request has Content-Length");
        if request.len() >= body_start.saturating_add(content_length) {
            return request;
        }
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
