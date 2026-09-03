//! Shared OpenAI-compatible codec seam and its bounded vocabulary.

use plexmaton_agent::{
    AdmissionRefusal, MAX_ADMITTED_ARGUMENT_BYTES, MAX_PROVIDER_REPLAY_BYTES, ModelError,
    ModelEvent, ModelRequest, ProviderReplayError, ToolCancellationReason, ToolOutcome,
};
use plexmaton_core::{TokenCounts, TokenUsage, ToolCallId};
use serde_json::Value;
use thiserror::Error;

use crate::{
    Protocol, ProviderProfile,
    chat::{self, ChatDecoder},
    responses::{self, ResponsesDecoder},
};

pub(crate) const RESPONSES_CODEC_ID: &str = "openai_responses";

/// Bounds enforced while one provider response is decoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeLimits {
    /// Total visible and plaintext-reasoning bytes retained by one step.
    pub max_retained_output_bytes: usize,
    /// Maximum bytes accepted in one SSE event before JSON decoding.
    pub max_sse_event_bytes: usize,
    /// Maximum raw JSON argument bytes accepted for one tool call.
    pub max_tool_argument_bytes: usize,
    /// Maximum complete tool calls accepted in one model step.
    pub max_tool_calls: usize,
    /// Maximum exact opaque replay bytes retained across one model step.
    pub max_replay_bytes: usize,
    /// Maximum opaque replay items retained across one model step.
    pub max_replay_items: usize,
    /// Maximum completed output items tracked across one model step.
    pub max_output_items: usize,
}

impl DecodeLimits {
    /// Builds the production limits for a validated provider profile.
    #[must_use]
    pub const fn for_profile(profile: &ProviderProfile) -> Self {
        Self {
            max_retained_output_bytes: profile.max_retained_output_bytes(),
            max_sse_event_bytes: 512 * 1024,
            max_tool_argument_bytes: MAX_ADMITTED_ARGUMENT_BYTES,
            max_tool_calls: 64,
            max_replay_bytes: MAX_PROVIDER_REPLAY_BYTES,
            max_replay_items: 16,
            max_output_items: 128,
        }
    }
}

/// One strict function schema made available to the provider.
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionTool {
    name: String,
    description: String,
    parameters: Value,
}

impl FunctionTool {
    /// Creates a named function whose parameters are a JSON Schema object.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
    ) -> Result<Self, FunctionToolError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(FunctionToolError::EmptyName);
        }
        if !parameters.is_object() {
            return Err(FunctionToolError::ParametersNotObject);
        }
        Ok(Self {
            name,
            description: description.into(),
            parameters,
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    pub(crate) const fn parameters(&self) -> &Value {
        &self.parameters
    }
}

/// A malformed tool definition refused before request construction.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FunctionToolError {
    #[error("a provider function tool must have a name")]
    EmptyName,
    #[error("a provider function tool's parameters must be a JSON object")]
    ParametersNotObject,
}

/// Why an authoritative semantic record could not be encoded for the selected dialect.
#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("plaintext Chat reasoning cannot be replayed through the Responses codec")]
    PlainReasoningInResponses,
    #[error("opaque provider replay cannot be sent through Chat Completions")]
    OpaqueReplayInChat,
    #[error("replay belongs to codec `{found}`, not `{expected}`")]
    WrongReplayCodec {
        found: String,
        expected: &'static str,
    },
    #[error("stored Responses replay is not valid JSON: {0}")]
    InvalidReplayJson(#[source] serde_json::Error),
    #[error("stored Responses replay is not a reasoning item")]
    InvalidReplayItem,
    #[error("a tool result `{0}` has no preceding provider call in the semantic record")]
    OrphanToolResult(String),
}

/// A provider stream that cannot be translated without guessing or dropping semantic content.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("provider event is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("provider event `{0}` is unsupported and may carry semantic content")]
    UnsupportedEvent(String),
    #[error("SSE event name `{event}` conflicts with JSON event type `{json_type}`")]
    ConflictingEventType { event: String, json_type: String },
    #[error("the provider emitted more than one terminal event")]
    DuplicateFinality,
    #[error("the provider stream ended before its terminal event")]
    IncompleteStream,
    #[error("tool call {index} has conflicting `{field}` fragments")]
    ConflictingToolFragment { index: usize, field: &'static str },
    #[error("tool call {index} completed without `{field}`")]
    IncompleteToolCall { index: usize, field: &'static str },
    #[error("tool call {index} arguments exceeded {limit} bytes")]
    ToolArgumentsTooLarge { index: usize, limit: usize },
    #[error("the response exceeded its {limit}-call tool bound")]
    TooManyToolCalls { limit: usize },
    #[error("the provider completed tool call id `{call_id}` more than once in one model step")]
    DuplicateToolCallId { call_id: ToolCallId },
    #[error("retained model output exceeded {limit} bytes")]
    RetainedOutputTooLarge { limit: usize },
    #[error("retained provider replay exceeded {limit} bytes")]
    RetainedReplayTooLarge { limit: usize },
    #[error("the response exceeded its {limit}-item replay bound")]
    TooManyReplayItems { limit: usize },
    #[error("the response exceeded its {limit}-item output bound")]
    TooManyOutputItems { limit: usize },
    #[error("output text {output_index}:{content_index} had conflicting `{field}` data")]
    ConflictingOutputText {
        output_index: usize,
        content_index: usize,
        field: &'static str,
    },
    #[error("output text {output_index}:{content_index} never completed")]
    IncompleteOutputText {
        output_index: usize,
        content_index: usize,
    },
    #[error("provider replay could not be retained: {0:?}")]
    Replay(ProviderReplayError),
    #[error("the provider failed the response with code {code:?}")]
    ProviderFailed { code: Option<String> },
    #[error("the provider reported an unknown completion reason `{0}`")]
    UnknownStopReason(String),
    #[error("provider usage field `{field}` is internally inconsistent")]
    InvalidUsage { field: &'static str },
    #[error("the provider reported usage more than once")]
    DuplicateUsage,
}

/// One explicitly selected dialect decoder. It cannot fall back or change protocol mid-stream.
#[derive(Debug)]
pub struct OpenAiCodec {
    decoder: Decoder,
}

#[derive(Debug)]
enum Decoder {
    Chat(ChatDecoder),
    Responses(ResponsesDecoder),
}

impl OpenAiCodec {
    /// Opens a fresh step decoder for one explicit protocol.
    #[must_use]
    pub fn new(protocol: Protocol, limits: DecodeLimits) -> Self {
        let decoder = match protocol {
            Protocol::Responses => Decoder::Responses(ResponsesDecoder::new(limits)),
            Protocol::ChatCompletions => Decoder::Chat(ChatDecoder::new(limits)),
        };
        Self { decoder }
    }

    /// Decodes one already-framed SSE event into ordered semantic events.
    pub fn push_sse(
        &mut self,
        event_name: &str,
        data: &str,
    ) -> Result<Vec<ModelEvent>, DecodeError> {
        match &mut self.decoder {
            Decoder::Chat(decoder) => decoder.push(event_name, data),
            Decoder::Responses(decoder) => decoder.push(event_name, data),
        }
    }

    /// Verifies that the byte stream ended after protocol finality and no partial call remains.
    pub fn finish(self) -> Result<(), DecodeError> {
        match self.decoder {
            Decoder::Chat(decoder) => decoder.finish(),
            Decoder::Responses(decoder) => decoder.finish(),
        }
    }
}

/// Rebuilds one stateless request from the semantic record for the selected profile.
pub fn encode_request(
    profile: &ProviderProfile,
    request: &ModelRequest,
    tools: &[FunctionTool],
    max_output_tokens: Option<u32>,
) -> Result<Value, EncodeError> {
    match profile.protocol() {
        Protocol::Responses => responses::encode(profile, request, tools, max_output_tokens),
        Protocol::ChatCompletions => chat::encode(profile, request, tools, max_output_tokens),
    }
}

/// Maps an HTTP failure without exposing provider bodies as machine-readable strings.
#[must_use]
pub fn classify_http_error(
    status: u16,
    retry_after_seconds: Option<u64>,
    body: &[u8],
) -> ModelError {
    if status == 429 {
        return ModelError::RateLimited {
            retry_after: retry_after_seconds,
        };
    }
    if body.len() <= 64 * 1024
        && error_code(body).is_some_and(|code| {
            matches!(
                code.as_str(),
                "context_length_exceeded" | "context_window_exceeded"
            )
        })
    {
        return ModelError::ContextTooLong;
    }
    if (200..300).contains(&status) {
        return ModelError::Malformed {
            message: format!("HTTP {status} response was not a valid provider stream"),
        };
    }
    ModelError::Transport {
        message: format!("provider returned HTTP {status}"),
    }
}

fn error_code(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("error")
        .and_then(|error| error.get("code").or_else(|| error.get("type")))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub(crate) fn retain_bytes(
    retained: &mut usize,
    added: usize,
    limit: usize,
) -> Result<(), DecodeError> {
    let Some(next) = retained.checked_add(added) else {
        return Err(DecodeError::RetainedOutputTooLarge { limit });
    };
    if next > limit {
        return Err(DecodeError::RetainedOutputTooLarge { limit });
    }
    *retained = next;
    Ok(())
}

pub(crate) fn reported_usage(counts: TokenCounts) -> Result<TokenUsage, DecodeError> {
    if counts
        .cached_input
        .is_some_and(|value| value > counts.input)
    {
        return Err(DecodeError::InvalidUsage {
            field: "cached_input",
        });
    }
    if counts
        .cache_write_input
        .is_some_and(|value| value > counts.input)
    {
        return Err(DecodeError::InvalidUsage {
            field: "cache_write_input",
        });
    }
    if counts
        .reasoning_output
        .is_some_and(|value| value > counts.output)
    {
        return Err(DecodeError::InvalidUsage {
            field: "reasoning_output",
        });
    }
    if counts.input.checked_add(counts.output) != Some(counts.total) {
        return Err(DecodeError::InvalidUsage { field: "total" });
    }
    let complete = counts.cached_input.is_some()
        && counts.cache_write_input.is_some()
        && counts.reasoning_output.is_some();
    Ok(if complete {
        TokenUsage::Complete(counts)
    } else {
        TokenUsage::Partial(counts)
    })
}

pub(crate) fn tool_output(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Succeeded { output } => output.clone(),
        ToolOutcome::Failed { message } => serde_json::json!({
            "status": "failed",
            "message": message,
        })
        .to_string(),
        ToolOutcome::AdmissionRefused { reason } => serde_json::json!({
            "status": "admission_refused",
            "reason": match reason {
                AdmissionRefusal::UnknownTool => "unknown_tool",
                AdmissionRefusal::InvalidArguments => "invalid_arguments",
                AdmissionRefusal::DefinitionUnavailable => "definition_unavailable",
                AdmissionRefusal::StalePrecondition => "stale_precondition",
                AdmissionRefusal::SourceMismatch => "source_mismatch",
                AdmissionRefusal::AmbiguousTarget => "ambiguous_target",
                AdmissionRefusal::ConflictingArguments => "conflicting_arguments",
                AdmissionRefusal::Cancelled => "cancelled",
            },
        })
        .to_string(),
        ToolOutcome::Forbidden => serde_json::json!({ "status": "forbidden" }).to_string(),
        ToolOutcome::Denied => serde_json::json!({ "status": "denied" }).to_string(),
        ToolOutcome::Cancelled { reason } => serde_json::json!({
            "status": "cancelled",
            "reason": match reason {
                ToolCancellationReason::Interrupted => "interrupted",
                ToolCancellationReason::StepFailed => "step_failed",
                ToolCancellationReason::Shutdown => "shutdown",
            },
        })
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_agent::{AdmissionRefusal, ModelError, ToolOutcome};

    use super::{classify_http_error, tool_output};

    #[test]
    fn typed_admission_refusals_keep_their_wire_reason() {
        for (reason, expected) in [
            (AdmissionRefusal::StalePrecondition, "stale_precondition"),
            (AdmissionRefusal::SourceMismatch, "source_mismatch"),
            (AdmissionRefusal::AmbiguousTarget, "ambiguous_target"),
            (
                AdmissionRefusal::ConflictingArguments,
                "conflicting_arguments",
            ),
            (AdmissionRefusal::Cancelled, "cancelled"),
        ] {
            let output = tool_output(&ToolOutcome::AdmissionRefused { reason });
            let output: serde_json::Value = serde_json::from_str(&output)
                .unwrap_or_else(|error| panic!("tool output JSON: {error}"));
            assert_eq!(output["reason"], expected);
        }
    }

    /// PRV-5: retry behavior is selected from status and typed metadata, never error prose.
    #[test]
    fn http_rate_limit_is_typed_and_keeps_retry_after() {
        assert_eq!(
            classify_http_error(429, Some(12), br#"{"error":{"message":"slow down"}}"#),
            ModelError::RateLimited {
                retry_after: Some(12)
            }
        );
    }

    /// PRV-5: context exhaustion is a stable category across provider presentation strings.
    #[test]
    fn context_error_is_classified_by_wire_code() {
        assert_eq!(
            classify_http_error(
                400,
                None,
                br#"{"error":{"code":"context_length_exceeded","message":"changeable"}}"#,
            ),
            ModelError::ContextTooLong
        );
    }

    #[test]
    fn arbitrary_provider_error_body_is_not_copied_into_diagnostics() {
        let error = classify_http_error(
            401,
            None,
            br#"{"error":{"message":"secret-shaped provider detail"}}"#,
        );
        assert_eq!(
            error,
            ModelError::Transport {
                message: "provider returned HTTP 401".to_owned()
            }
        );
    }
}
