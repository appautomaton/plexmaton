//! Pooled provider HTTP transport for one resolved model.

use std::{sync::Arc, time::Duration};

use futures_util::{FutureExt, StreamExt, future::BoxFuture};
use plexmaton_agent::{ModelCall, ModelError, ModelEvent, RequestAttemptId, RequestEnvironment};
use plexmaton_core::TokenUsage;
use plexmaton_provider::{
    ApiKey, DecodeLimits, FunctionTool, ModelApi, ResolvedModel, SseDecodeError,
    classify_http_error, drive_sse, encode_request, request_cost, request_environment,
};
use reqwest::{Client, Url, header};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::runtime::{
    ModelCompletion, ModelDriver, ModelOutput, ModelSignal, ModelTerminalReport, WallClock,
};

mod timing;
#[cfg(test)]
mod timing_tests;

use timing::{RequestTimer, not_dispatched_report};

const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// Invalid transport configuration rejected before terminal or network ownership (LIVE-6).
#[derive(Debug, Error)]
pub enum HttpSetupError {
    #[error("native tool credential environment does not match the selected provider")]
    ToolCredentialEnvironmentMismatch,
    #[error("provider base URL is invalid: {0}")]
    InvalidBaseUrl(String),
    #[error("provider base URL cannot contain credentials, a query, or a fragment")]
    UnsafeBaseUrl,
    #[error("provider base URL uses unsupported scheme `{0}`")]
    UnsupportedScheme(String),
    #[error("provider endpoint could not be constructed")]
    InvalidEndpoint,
    #[error("HTTP client could not be constructed: {0}")]
    Client(#[source] reqwest::Error),
}

pub(crate) struct ProviderHttp {
    client: Client,
    endpoint: Url,
    model: ResolvedModel,
    key: Arc<ApiKey>,
    tools: Arc<[FunctionTool]>,
    environment: RequestEnvironment,
    clock: Arc<dyn WallClock>,
}

impl ProviderHttp {
    pub(crate) fn new(
        model: ResolvedModel,
        key: ApiKey,
        tools: Arc<[FunctionTool]>,
        clock: Arc<dyn WallClock>,
    ) -> Result<Self, HttpSetupError> {
        let endpoint = endpoint(&model)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(HttpSetupError::Client)?;
        let environment = request_environment(&model, &tools, Some(model.max_output_tokens()));
        Ok(Self {
            client,
            endpoint,
            model,
            key: Arc::new(key),
            tools,
            environment,
            clock,
        })
    }

    fn authenticated_request(&self) -> reqwest::RequestBuilder {
        let request = self
            .client
            .post(self.endpoint.clone())
            .header(header::ACCEPT, "text/event-stream");
        match self.model.api() {
            ModelApi::OpenaiResponses | ModelApi::OpenaiChatCompletions => {
                request.bearer_auth(self.key.expose())
            }
            ModelApi::AnthropicMessages => request
                .header("x-api-key", self.key.expose())
                .header("anthropic-version", "2023-06-01"),
            ModelApi::GoogleGenerateContent => request.header("x-goog-api-key", self.key.expose()),
        }
    }

    async fn perform(
        &self,
        attempt_id: RequestAttemptId,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        cancellation: CancellationToken,
    ) -> ModelTerminalReport {
        if cancellation.is_cancelled() {
            return not_dispatched_report(
                attempt_id,
                call.step_id,
                plexmaton_agent::RequestNotDispatchedOutcome::Cancelled,
                ModelCompletion::Cancelled,
            );
        }
        let body = match encode_request(
            &self.model,
            &call.request,
            &self.tools,
            Some(self.model.max_output_tokens()),
        ) {
            Ok(body) => body,
            Err(error) => {
                return not_dispatched_report(
                    attempt_id,
                    call.step_id,
                    plexmaton_agent::RequestNotDispatchedOutcome::EncodingFailed,
                    ModelCompletion::Failed(ModelError::Malformed {
                        message: error.to_string(),
                    }),
                );
            }
        };
        if cancellation.is_cancelled() {
            return not_dispatched_report(
                attempt_id,
                call.step_id,
                plexmaton_agent::RequestNotDispatchedOutcome::Cancelled,
                ModelCompletion::Cancelled,
            );
        }
        let request = self.authenticated_request().json(&body);
        let mut timer = RequestTimer::start(self.clock.now());
        let response = {
            let send = request.send();
            tokio::pin!(send);
            tokio::select! {
                biased;
                response = &mut send => response,
                () = cancellation.cancelled() => {
                    return timer.cancelled(
                        attempt_id,
                        call.step_id,
                        TokenUsage::Unavailable,
                    );
                }
            }
        };
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                return timer.failed(
                    attempt_id,
                    call.step_id,
                    ModelError::Transport {
                        message: error.to_string(),
                    },
                    TokenUsage::Unavailable,
                );
            }
        };
        timer.headers_arrived();
        if !response.status().is_success() {
            return failed_response_report(
                timer,
                attempt_id,
                call.step_id,
                response,
                &cancellation,
            )
            .await;
        }

        let limits = DecodeLimits::production();
        let retry_after = retry_after_seconds(&response);
        let step_id = call.step_id.clone();
        let stream = response.bytes_stream();
        let mut usage = None;
        let mut stop = None;
        enum DecodeResult<E> {
            Finished(Result<(), SseDecodeError<E>>),
            Cancelled,
        }
        let decoded = {
            let decoded = drive_sse(&attempt_id, &self.model, stream, limits, |event| {
                let signal = match event {
                    ModelEvent::Usage(report) => {
                        usage = Some(report);
                        None
                    }
                    ModelEvent::Stopped(reason) => {
                        stop = Some(reason);
                        None
                    }
                    event => {
                        let output = ModelOutput::from_event(event).unwrap_or_else(|_| {
                            unreachable!("usage and stop were handled before model output")
                        });
                        timer.output_arrived(&output);
                        Some(ModelSignal {
                            attempt_id: attempt_id.clone(),
                            step_id: step_id.clone(),
                            output,
                        })
                    }
                };
                let signals = signals.clone();
                async move {
                    if let Some(signal) = signal {
                        let _closed = signals.send(signal).await;
                    }
                }
            });
            tokio::pin!(decoded);
            tokio::select! {
                biased;
                result = &mut decoded => DecodeResult::Finished(result),
                () = cancellation.cancelled() => DecodeResult::Cancelled,
            }
        };
        let usage = usage.unwrap_or(TokenUsage::Unavailable);
        match decoded {
            DecodeResult::Cancelled => timer.cancelled(attempt_id, call.step_id, usage),
            DecodeResult::Finished(Err(error)) => timer.failed(
                attempt_id,
                call.step_id,
                error.into_model_error(retry_after),
                usage,
            ),
            DecodeResult::Finished(Ok(())) => {
                // TIM-3: a complete field breakdown does not make an interim snapshot a final bill.
                let cost = request_cost(&self.model, &usage);
                let reason = stop.unwrap_or_else(|| {
                    unreachable!("a successful SSE drive always emits its retained stop")
                });
                timer.completed(attempt_id, call.step_id, reason, usage, cost)
            }
        }
    }
}

async fn failed_response_report(
    timer: RequestTimer,
    attempt_id: RequestAttemptId,
    step_id: plexmaton_agent::ModelStepId,
    response: reqwest::Response,
    cancellation: &CancellationToken,
) -> ModelTerminalReport {
    let status = response.status().as_u16();
    let retry_after = retry_after_seconds(&response);
    let body = bounded_error_body(response);
    tokio::pin!(body);
    tokio::select! {
        biased;
        body = &mut body => timer.failed(
            attempt_id,
            step_id,
            classify_http_error(status, retry_after, &body),
            TokenUsage::Unavailable,
        ),
        () = cancellation.cancelled() => {
            timer.cancelled(
                attempt_id,
                step_id,
                TokenUsage::Unavailable,
            )
        }
    }
}

impl ModelDriver for ProviderHttp {
    fn request_environment(&self) -> &RequestEnvironment {
        &self.environment
    }

    fn budget_inputs(&self) -> Option<(&ResolvedModel, &[FunctionTool])> {
        Some((&self.model, &self.tools))
    }

    fn drive(
        &self,
        attempt_id: RequestAttemptId,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, ModelTerminalReport> {
        let this = Self {
            client: self.client.clone(),
            endpoint: self.endpoint.clone(),
            model: self.model.clone(),
            key: Arc::clone(&self.key),
            tools: Arc::clone(&self.tools),
            environment: self.environment.clone(),
            clock: Arc::clone(&self.clock),
        };
        async move { this.perform(attempt_id, call, signals, cancellation).await }.boxed()
    }
}

fn endpoint(model: &ResolvedModel) -> Result<Url, HttpSetupError> {
    let mut base = Url::parse(model.base_url())
        .map_err(|error| HttpSetupError::InvalidBaseUrl(error.to_string()))?;
    if !matches!(base.scheme(), "http" | "https") {
        return Err(HttpSetupError::UnsupportedScheme(base.scheme().to_owned()));
    }
    if !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(HttpSetupError::UnsafeBaseUrl);
    }
    if !base.path().ends_with('/') {
        let mut path = base.path().to_owned();
        path.push('/');
        base.set_path(&path);
    }
    if model.api() == ModelApi::GoogleGenerateContent {
        let mut endpoint = base
            .join(&format!("models/{}:streamGenerateContent", model.wire_id()))
            .map_err(|_| HttpSetupError::InvalidEndpoint)?;
        endpoint.set_query(Some("alt=sse"));
        return Ok(endpoint);
    }
    let resource = match model.api() {
        ModelApi::OpenaiResponses => "responses",
        ModelApi::OpenaiChatCompletions => "chat/completions",
        ModelApi::AnthropicMessages => "messages",
        ModelApi::GoogleGenerateContent => unreachable!("GenerateContent endpoint was returned"),
    };
    base.join(resource)
        .map_err(|_| HttpSetupError::InvalidEndpoint)
}

async fn bounded_error_body(response: reqwest::Response) -> Vec<u8> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            break;
        };
        let room = MAX_ERROR_BODY_BYTES
            .saturating_add(1)
            .saturating_sub(body.len());
        if room == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
    body
}

fn retry_after_seconds(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Arc};

    use plexmaton_agent::ModelRequest;
    use plexmaton_core::AgentId;
    use plexmaton_provider::{
        ModelApi, ModelRegistry, ResolvedModel, encode_request, resolve_api_key,
    };

    use super::{HttpSetupError, ProviderHttp, endpoint};
    use crate::{LiveRuntime, NativeToolCatalog};

    fn model(base_url: &str, api: ModelApi) -> ResolvedModel {
        let api = match api {
            ModelApi::OpenaiResponses => "openai_responses",
            ModelApi::OpenaiChatCompletions => "openai_chat_completions",
            ModelApi::AnthropicMessages => "anthropic_messages",
            ModelApi::GoogleGenerateContent => "google_generate_content",
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

    /// LIVE-6: endpoint validation finishes before the terminal or a request can be owned.
    #[test]
    fn endpoint_resolution_is_api_specific() {
        assert_eq!(
            endpoint(&model(
                "http://127.0.0.1:8317/v1",
                ModelApi::OpenaiResponses
            ))
            .expect("responses endpoint")
            .as_str(),
            "http://127.0.0.1:8317/v1/responses"
        );
        assert_eq!(
            endpoint(&model(
                "http://127.0.0.1:8317/v1/",
                ModelApi::OpenaiChatCompletions
            ))
            .expect("chat endpoint")
            .as_str(),
            "http://127.0.0.1:8317/v1/chat/completions"
        );
    }

    /// BUD-1: the production runtime exposes the same ledger as its exact provider inputs.
    #[tokio::test]
    async fn bud_1_runtime_snapshot_uses_the_configured_model_without_dispatch() {
        use crate::{ContextBudgetSnapshot, LiveRuntime};
        use plexmaton_agent::Input;

        let model = model("http://127.0.0.1:1/v1", ModelApi::OpenaiResponses);
        let catalog = NativeToolCatalog::open(
            std::env::current_dir().expect("workspace"),
            "TEST_KEY",
            "/bin/false",
            "/bin/false",
            Vec::new(),
        )
        .expect("catalog");
        let tools = catalog.provider_definitions();
        let key = resolve_api_key(&model, Some("unused-test-key".into())).expect("key");
        let mut runtime = LiveRuntime::provider(
            AgentId::new("agent-budget").expect("id"),
            "Agent",
            model.clone(),
            key,
            catalog,
        )
        .expect("runtime");
        runtime
            .submit(
                runtime.agent_id().clone(),
                Input::Submitted {
                    text: "budget this without sending HTTP".to_owned(),
                },
            )
            .await
            .expect("submit");
        let ContextBudgetSnapshot::Available(snapshot) =
            runtime.context_budget().expect("snapshot")
        else {
            panic!("configured model has budget");
        };
        // No next_update() is polled: the model future remains unstarted.
        assert!(snapshot.anchor.is_none());
        assert_eq!(snapshot.atoms.len(), 1);
        assert_eq!(
            snapshot.environment,
            plexmaton_provider::request_environment(
                &model,
                &tools,
                Some(model.max_output_tokens())
            )
        );
        assert!(snapshot.environment_estimate.tokens > 0);
        assert_eq!(
            snapshot.limits.context_window_tokens(),
            u64::from(model.context_window_tokens())
        );
        runtime.shutdown().await.expect("cancel before dispatch");
    }

    /// LIVE-1/PRV-1: the live HTTP edge publishes one exact, unique native catalog through either
    /// selected wire dialect; neither dialect invents or loses a definition.
    #[test]
    fn native_catalog_is_exact_unique_and_advertised_by_both_protocols() {
        let workspace = std::env::current_dir()
            .unwrap_or_else(|error| panic!("resolve test workspace: {error}"));
        let catalog = NativeToolCatalog::open(
            workspace,
            "TEST_KEY",
            "/bin/false",
            "/bin/false",
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("open native catalog: {error}"));
        let definitions = catalog.provider_definitions();
        let request = ModelRequest {
            session_id: plexmaton_core::ConversationId::new("fixture-session")
                .unwrap_or_else(|error| panic!("session: {error}")),
            atoms: Vec::new(),
        };
        let mut bodies = Vec::new();

        for api in [ModelApi::OpenaiResponses, ModelApi::OpenaiChatCompletions] {
            let model = model("http://127.0.0.1:8317/v1", api);
            let key = resolve_api_key(&model, Some("fixture-secret".into()))
                .unwrap_or_else(|error| panic!("resolve fixture key: {error}"));
            let clock = Arc::new(crate::runtime::FixedWallClock(
                plexmaton_agent::UnixMillis::new(100),
            ));
            let http = ProviderHttp::new(model, key, definitions.clone(), clock)
                .unwrap_or_else(|error| panic!("open HTTP edge: {error}"));
            bodies.push(
                encode_request(
                    &http.model,
                    &request,
                    &http.tools,
                    Some(http.model.max_output_tokens()),
                )
                .unwrap_or_else(|error| panic!("encode native catalog: {error}")),
            );
        }

        let responses = bodies[0]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("Responses tools must be an array"));
        let chat = bodies[1]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("Chat tools must be an array"));
        let expected = [
            "read_file",
            "search",
            "edit_file",
            "create_file",
            "exec_command",
        ];
        let response_names: Vec<_> = responses
            .iter()
            .map(|tool| tool["name"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(response_names, expected);
        assert_eq!(
            response_names
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len(),
            expected.len(),
            "provider-visible tool names must be unique"
        );
        assert_eq!(chat.len(), expected.len());

        for (response_tool, chat_tool) in responses.iter().zip(chat) {
            let chat_function = &chat_tool["function"];
            assert_eq!(chat_function["name"], response_tool["name"]);
            assert_eq!(chat_function["description"], response_tool["description"]);
            assert_eq!(chat_function["parameters"], response_tool["parameters"]);
            assert_eq!(response_tool["type"], "function");
            assert_eq!(chat_tool["type"], "function");
            assert_eq!(response_tool["strict"], true);
            assert_eq!(chat_function["strict"], true);
            assert_eq!(response_tool["parameters"]["type"], "object");
            assert_eq!(response_tool["parameters"]["additionalProperties"], false);
            assert!(
                response_tool["description"]
                    .as_str()
                    .is_some_and(|description| !description.is_empty())
            );
        }
        assert_eq!(bodies[0]["tool_choice"], "auto");
        assert_eq!(bodies[1]["tool_choice"], "auto");
        assert_eq!(bodies[0]["max_output_tokens"], 128_000);
        assert_eq!(bodies[1]["max_completion_tokens"], 128_000);
    }

    /// LIVE-6: the runtime cannot combine one provider with another credential environment.
    #[test]
    fn catalog_key_identity_must_match_resolved_model() {
        let workspace = std::env::current_dir()
            .unwrap_or_else(|error| panic!("resolve test workspace: {error}"));
        let catalog = NativeToolCatalog::open(
            workspace,
            "OTHER_KEY",
            "/bin/false",
            "/bin/false",
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("open mismatched catalog: {error}"));
        let model = model("http://127.0.0.1:8317/v1", ModelApi::OpenaiResponses);
        let key = resolve_api_key(&model, Some("fixture-secret".into()))
            .unwrap_or_else(|error| panic!("resolve fixture key: {error}"));
        let agent = AgentId::new("catalog-key-fixture")
            .unwrap_or_else(|error| panic!("fixture agent id: {error}"));

        assert!(matches!(
            LiveRuntime::provider(agent, "fixture", model, key, catalog),
            Err(crate::RuntimeError::HttpSetup(
                HttpSetupError::ToolCredentialEnvironmentMismatch
            ))
        ));
    }
}
