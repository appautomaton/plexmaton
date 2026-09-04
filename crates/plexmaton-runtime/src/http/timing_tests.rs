use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    sync::{Arc, mpsc as std_mpsc},
    thread,
    time::Duration,
};

use plexmaton_agent::{
    Agent, AssistantBlock, AssistantOutput, AssistantReplay, ContextAtom, Effect, Input, ModelCall,
    ModelRequest, ProviderReplay, RequestAttemptId, RequestAttemptTerminalState,
    RequestDispatchedOutcome, RequestNotDispatchedOutcome, UnixMillis,
};
use plexmaton_core::{AgentId, SessionEntryId, TokenUsage, TranscriptItemId};
use plexmaton_provider::{
    FunctionTool, ModelApi, ModelRegistry, ResolvedModel, request_environment, resolve_api_key,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::OpenAiHttp;
use crate::runtime::{FixedWallClock, ModelCompletion, ModelDriver};

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
        ModelCompletion::Failed(plexmaton_agent::ModelError::Malformed { .. })
    ));
    assert!(received.try_recv().is_err());
}

/// TIM-2/TIM-3: one dispatched request owns ordered milestones and terminal-only exact usage.
#[tokio::test]
async fn dispatched_response_returns_one_correlated_terminal_report() {
    let (base_url, server) = responding_server(FINAL_ANSWER);
    let fixed_wall = UnixMillis::new(1_788_000_000_123);
    let http = http(&base_url, ModelApi::OpenaiResponses, fixed_wall);
    let call = model_call();
    let step_id = call.step_id.clone();
    let attempt_id = attempt();
    let (signals, mut received) = mpsc::channel(16);

    let report = http
        .drive(attempt_id.clone(), call, signals, CancellationToken::new())
        .await;
    let request = server
        .join()
        .unwrap_or_else(|_| panic!("fixture server panicked"))
        .unwrap_or_else(|error| panic!("fixture server: {error}"));
    assert!(String::from_utf8_lossy(&request).contains("POST /v1/responses"));
    assert_eq!(report.step_id, step_id);
    assert_eq!(report.terminal.attempt_id(), &attempt_id);
    let RequestAttemptTerminalState::Dispatched {
        timing,
        outcome,
        usage,
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
    assert!(matches!(report.completion, ModelCompletion::Stopped(_)));

    let mut outputs = Vec::new();
    while let Ok(signal) = received.try_recv() {
        assert_eq!(signal.attempt_id, attempt_id);
        assert_eq!(signal.step_id, step_id);
        outputs.push(signal.output.into_event());
    }
    assert!(!outputs.is_empty());
    assert!(outputs.iter().all(|event| !matches!(
        event,
        plexmaton_agent::ModelEvent::Usage(_) | plexmaton_agent::ModelEvent::Stopped(_)
    )));
}

/// TIM-2/TIM-5: cancellation after the server observes a request stays dispatched even without
/// response headers.
#[tokio::test]
async fn cancellation_after_dispatch_keeps_only_observed_milestones() {
    let (base_url, observed, release, server) = holding_server();
    let fixed_wall = UnixMillis::new(1_788_000_000_456);
    let http = http(&base_url, ModelApi::OpenaiResponses, fixed_wall);
    let cancellation = CancellationToken::new();
    let (signals, mut received) = mpsc::channel(8);
    let future = http.drive(attempt(), model_call(), signals, cancellation.child_token());
    let task = tokio::spawn(future);
    observed
        .await
        .unwrap_or_else(|_| panic!("fixture server did not observe request"));
    cancellation.cancel();
    let report = task
        .await
        .unwrap_or_else(|error| panic!("request task: {error}"));
    release
        .send(())
        .unwrap_or_else(|_| panic!("release fixture server"));
    server
        .join()
        .unwrap_or_else(|_| panic!("fixture server panicked"))
        .unwrap_or_else(|error| panic!("fixture server: {error}"));

    let RequestAttemptTerminalState::Dispatched {
        timing,
        outcome,
        usage,
    } = report.terminal.terminal()
    else {
        panic!("observed request crossed dispatch boundary");
    };
    assert_eq!(timing.dispatched_at(), fixed_wall);
    assert_eq!(timing.headers_after_ms(), None);
    assert_eq!(timing.first_output_after_ms(), None);
    assert!(matches!(outcome, RequestDispatchedOutcome::Cancelled));
    assert_eq!(usage, &TokenUsage::Unavailable);
    assert!(matches!(report.completion, ModelCompletion::Cancelled));
    assert!(received.try_recv().is_err());
}

type ServerResult = Result<Vec<u8>, String>;

fn responding_server(body: &'static str) -> (String, thread::JoinHandle<ServerResult>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("bind fixture server: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("fixture address: {error}"));
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .map_err(|error| format!("accept request: {error}"))?;
        let request = read_request(&mut stream)?;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(headers.as_bytes())
            .and_then(|()| stream.write_all(body.as_bytes()))
            .map_err(|error| format!("write response: {error}"))?;
        Ok(request)
    });
    (format!("http://{address}/v1"), handle)
}

type HoldingServer = (
    String,
    oneshot::Receiver<()>,
    std_mpsc::Sender<()>,
    thread::JoinHandle<Result<(), String>>,
);

fn holding_server() -> HoldingServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("bind fixture server: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("fixture address: {error}"));
    let (observed_tx, observed_rx) = oneshot::channel();
    let (release_tx, release_rx) = std_mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .map_err(|error| format!("accept request: {error}"))?;
        let _request = read_request(&mut stream)?;
        observed_tx
            .send(())
            .map_err(|_| "request observer dropped".to_owned())?;
        release_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| format!("wait for release: {error}"))?;
        Ok(())
    });
    (
        format!("http://{address}/v1"),
        observed_rx,
        release_tx,
        handle,
    )
}

fn read_request(stream: &mut TcpStream) -> ServerResult {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| format!("set read timeout: {error}"))?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| format!("read request: {error}"))?;
        if count == 0 {
            return Err("request ended before its body".to_owned());
        }
        request.extend_from_slice(&buffer[..count]);
        let Some(header_end) = find_bytes(&request, b"\r\n\r\n") else {
            continue;
        };
        let body_start = header_end + 4;
        let headers = std::str::from_utf8(&request[..header_end])
            .map_err(|error| format!("request headers were not UTF-8: {error}"))?;
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .ok_or_else(|| "request omitted Content-Length".to_owned())?;
        if request.len() >= body_start.saturating_add(content_length) {
            return Ok(request);
        }
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
