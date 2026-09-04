//! Pooled OpenAI-compatible HTTP transport for one resolved model.

use std::{sync::Arc, time::Duration};

use futures_util::{FutureExt, StreamExt, future::BoxFuture};
use plexmaton_agent::{ModelCall, ModelError, ModelEvent};
use plexmaton_provider::{
    ApiKey, DecodeLimits, FunctionTool, ModelApi, ResolvedModel, SseDecodeError,
    classify_http_error, drive_sse, encode_request,
};
use reqwest::{Client, Url, header};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::runtime::{ModelDriver, ModelSignal};

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

pub(crate) struct OpenAiHttp {
    client: Client,
    endpoint: Url,
    model: ResolvedModel,
    key: Arc<ApiKey>,
    tools: Arc<[FunctionTool]>,
}

impl OpenAiHttp {
    pub(crate) fn new(
        model: ResolvedModel,
        key: ApiKey,
        tools: Arc<[FunctionTool]>,
    ) -> Result<Self, HttpSetupError> {
        let endpoint = endpoint(&model)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(HttpSetupError::Client)?;
        Ok(Self {
            client,
            endpoint,
            model,
            key: Arc::new(key),
            tools,
        })
    }

    async fn perform(&self, call: ModelCall, signals: mpsc::Sender<ModelSignal>) {
        let body = match encode_request(
            &self.model,
            &call.request,
            &self.tools,
            Some(self.model.max_output_tokens()),
        ) {
            Ok(body) => body,
            Err(error) => {
                send_failure(
                    &signals,
                    call.step_id,
                    ModelError::Malformed {
                        message: error.to_string(),
                    },
                )
                .await;
                return;
            }
        };
        let response = match self
            .client
            .post(self.endpoint.clone())
            .header(header::ACCEPT, "text/event-stream")
            .bearer_auth(self.key.expose())
            .json(&body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                send_failure(
                    &signals,
                    call.step_id,
                    ModelError::Transport {
                        message: error.to_string(),
                    },
                )
                .await;
                return;
            }
        };
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok());
            let body = bounded_error_body(response).await;
            send_failure(
                &signals,
                call.step_id,
                classify_http_error(status, retry_after, &body),
            )
            .await;
            return;
        }

        let limits = DecodeLimits::production();
        let step_id = call.step_id;
        let stream = response.bytes_stream();
        let decoded = drive_sse(&self.model, stream, limits, |event| {
            let signals = signals.clone();
            let step_id = step_id.clone();
            async move {
                let signal = if matches!(event, ModelEvent::Stopped(_)) {
                    ModelSignal::Terminal { step_id, event }
                } else {
                    ModelSignal::Event { step_id, event }
                };
                let _closed = signals.send(signal).await;
            }
        })
        .await;
        if let Err(error) = decoded {
            send_failure(&signals, step_id, decode_error(error)).await;
        }
    }
}

impl ModelDriver for OpenAiHttp {
    fn drive(
        &self,
        call: ModelCall,
        signals: mpsc::Sender<ModelSignal>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, ()> {
        let this = Self {
            client: self.client.clone(),
            endpoint: self.endpoint.clone(),
            model: self.model.clone(),
            key: Arc::clone(&self.key),
            tools: Arc::clone(&self.tools),
        };
        async move {
            tokio::select! {
                () = cancellation.cancelled() => {}
                () = this.perform(call, signals) => {}
            }
        }
        .boxed()
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
    let resource = match model.api() {
        ModelApi::OpenaiResponses => "responses",
        ModelApi::OpenaiChatCompletions => "chat/completions",
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

fn decode_error(error: SseDecodeError<reqwest::Error>) -> ModelError {
    let message = error.to_string();
    match error {
        SseDecodeError::Transport(error) => ModelError::Transport {
            message: error.to_string(),
        },
        SseDecodeError::InvalidUtf8
        | SseDecodeError::EventTooLarge { .. }
        | SseDecodeError::Decode(_) => ModelError::Malformed { message },
    }
}

async fn send_failure(
    signals: &mpsc::Sender<ModelSignal>,
    step_id: plexmaton_agent::ModelStepId,
    error: ModelError,
) {
    let _closed = signals.send(ModelSignal::Failed { step_id, error }).await;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use plexmaton_agent::ModelRequest;
    use plexmaton_core::AgentId;
    use plexmaton_provider::{
        ModelApi, ModelRegistry, ResolvedModel, encode_request, resolve_api_key,
    };

    use super::{HttpSetupError, OpenAiHttp, endpoint};
    use crate::{LiveRuntime, NativeToolCatalog};

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
        let request = ModelRequest { atoms: Vec::new() };
        let mut bodies = Vec::new();

        for api in [ModelApi::OpenaiResponses, ModelApi::OpenaiChatCompletions] {
            let model = model("http://127.0.0.1:8317/v1", api);
            let key = resolve_api_key(&model, Some("fixture-secret".into()))
                .unwrap_or_else(|error| panic!("resolve fixture key: {error}"));
            let http = OpenAiHttp::new(model, key, definitions.clone())
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
            LiveRuntime::openai(agent, "fixture", model, key, catalog),
            Err(crate::RuntimeError::HttpSetup(
                HttpSetupError::ToolCredentialEnvironmentMismatch
            ))
        ));
    }
}
