//! Pooled OpenAI-compatible HTTP transport for one selected provider profile.

use std::{sync::Arc, time::Duration};

use futures_util::{FutureExt, StreamExt, future::BoxFuture};
use plexmaton_agent::{ModelCall, ModelError, ModelEvent};
use plexmaton_provider::{
    ApiKey, DecodeLimits, Protocol, ProviderProfile, SseDecodeError, classify_http_error,
    drive_sse, encode_request,
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
    profile: ProviderProfile,
    key: Arc<ApiKey>,
}

impl OpenAiHttp {
    pub(crate) fn new(profile: ProviderProfile, key: ApiKey) -> Result<Self, HttpSetupError> {
        let endpoint = endpoint(&profile)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(HttpSetupError::Client)?;
        Ok(Self {
            client,
            endpoint,
            profile,
            key: Arc::new(key),
        })
    }

    async fn perform(&self, call: ModelCall, signals: mpsc::Sender<ModelSignal>) {
        let body = match encode_request(&self.profile, &call.request, &[], None) {
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

        let protocol = self.profile.protocol();
        let limits = DecodeLimits::for_profile(&self.profile);
        let step_id = call.step_id;
        let stream = response.bytes_stream();
        let decoded = drive_sse(protocol, stream, limits, |event| {
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
            profile: self.profile.clone(),
            key: Arc::clone(&self.key),
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

fn endpoint(profile: &ProviderProfile) -> Result<Url, HttpSetupError> {
    let mut base = Url::parse(profile.base_url())
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
    let resource = match profile.protocol() {
        Protocol::Responses => "responses",
        Protocol::ChatCompletions => "chat/completions",
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
    use plexmaton_provider::{Protocol, ProviderConfig};

    use super::{HttpSetupError, endpoint};

    fn profile(base_url: &str, protocol: Protocol) -> plexmaton_provider::ProviderProfile {
        let protocol = match protocol {
            Protocol::Responses => "responses",
            Protocol::ChatCompletions => "chat_completions",
        };
        ProviderConfig::parse(&format!(
            r#"
active_provider = "test"
[providers.test]
kind = "openai_compatible"
protocol = "{protocol}"
base_url = "{base_url}"
model = "gpt-5.6-luna"
api_key_env = "TEST_KEY"
reasoning_effort = "low"
"#
        ))
        .unwrap_or_else(|error| panic!("fixture profile: {error}"))
        .active()
        .clone()
    }

    /// LIVE-6: endpoint validation finishes before the terminal or a request can be owned.
    #[test]
    fn endpoint_resolution_is_protocol_specific_and_rejects_embedded_authority() {
        assert_eq!(
            endpoint(&profile("http://127.0.0.1:8317/v1", Protocol::Responses))
                .expect("responses endpoint")
                .as_str(),
            "http://127.0.0.1:8317/v1/responses"
        );
        assert_eq!(
            endpoint(&profile(
                "http://127.0.0.1:8317/v1/",
                Protocol::ChatCompletions
            ))
            .expect("chat endpoint")
            .as_str(),
            "http://127.0.0.1:8317/v1/chat/completions"
        );
        assert!(matches!(
            endpoint(&profile(
                "http://name:password@127.0.0.1:8317/v1",
                Protocol::Responses
            )),
            Err(HttpSetupError::UnsafeBaseUrl)
        ));
        assert!(matches!(
            endpoint(&profile("ftp://example.test/v1", Protocol::Responses)),
            Err(HttpSetupError::UnsupportedScheme(scheme)) if scheme == "ftp"
        ));
    }
}
