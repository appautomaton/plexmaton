//! The narrow model surface the loop is written against.
//!
//! These types are the loop's, not a provider's. An adapter translates one dialect's wire form
//! into them and keeps its own vocabulary — response ids, routing metadata, moderation fields — on
//! its own side of the boundary. A shared event type carrying one dialect's concerns leaves every
//! other adapter fabricating fields it does not have.

use std::fmt;

use plexmaton_core::{TokenUsage, ToolCallId, TurnId};

use crate::tools::{ToolCall, ToolOutcome};

/// Stable identity of one model request within a turn (LIVE-2).
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ModelStepId {
    turn_id: TurnId,
    index: u16,
}

impl ModelStepId {
    pub(crate) const fn new(turn_id: TurnId, index: u16) -> Self {
        Self { turn_id, index }
    }

    /// Turn that owns this request.
    #[must_use]
    pub const fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }

    /// One-based position of this model request within its turn.
    #[must_use]
    pub const fn index(&self) -> u16 {
        self.index
    }
}

/// What the loop needs the model to be asked.
///
/// The whole conversation, assembled by the session that owns it. Assembling per step rather than
/// borrowing is deliberate for now: correctness first, and the cost is one clone per model request
/// rather than per delta. When it is measured and matters, the fix is to borrow the history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelRequest {
    /// The conversation so far, oldest first.
    pub items: Vec<RequestItem>,
}

/// One correlated request the runtime must perform.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCall {
    /// Stable identity every resulting event or failure must echo.
    pub step_id: ModelStepId,
    /// Stateless semantic conversation to encode.
    pub request: ModelRequest,
}

/// Maximum opaque provider replay bytes retained for one item.
pub const MAX_PROVIDER_REPLAY_BYTES: usize = 256 * 1024;

/// Stable identity of the codec that can interpret an opaque replay item.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProviderCodecId(String);

impl ProviderCodecId {
    /// Creates a non-empty codec identity.
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderReplayError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ProviderReplayError::EmptyCodec);
        }
        Ok(Self(value))
    }

    /// Returns the codec's stable external name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderCodecId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Why an opaque replay item was refused before entering turn state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderReplayError {
    /// A replay item without its codec cannot be interpreted later.
    EmptyCodec,
    /// The exact payload exceeded the hard retained-state bound.
    PayloadTooLarge,
}

/// Exact provider data required to reconstruct a later request.
///
/// The loop retains and orders this value but never interprets `payload`. Only the named codec may
/// decode it, which keeps encrypted reasoning out of semantic text while leaving replay state
/// inspectable (LOOP-4, PRV-3).
#[derive(Clone, Eq, PartialEq)]
pub struct ProviderReplay {
    codec: ProviderCodecId,
    payload: String,
}

impl fmt::Debug for ProviderReplay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderReplay")
            .field("codec", &self.codec)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}

impl ProviderReplay {
    /// Builds one bounded opaque replay item.
    pub fn new(codec: ProviderCodecId, payload: String) -> Result<Self, ProviderReplayError> {
        if payload.len() > MAX_PROVIDER_REPLAY_BYTES {
            return Err(ProviderReplayError::PayloadTooLarge);
        }
        Ok(Self { codec, payload })
    }

    /// Codec that owns the payload's wire meaning.
    #[must_use]
    pub const fn codec(&self) -> &ProviderCodecId {
        &self.codec
    }

    /// Exact bounded payload. Presentation code must never render or log it.
    #[must_use]
    pub fn payload(&self) -> &str {
        &self.payload
    }
}

/// One entry of the conversation as the model is shown it.
///
/// A step where the model both spoke and asked for tools leaves an [`Self::Assistant`] entry
/// followed by its [`Self::ToolCall`] entries. Whether a dialect sends those as one message with
/// several blocks or as separate turns is the adapter's business, and exactly the kind of thing
/// that must not reach this far in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestItem {
    /// Something the person said.
    User {
        /// Exact text the user submitted.
        text: String,
    },
    /// Something the model said, as it was assembled from the stream.
    Assistant {
        /// The finished text of one assistant message.
        text: String,
    },
    /// Plain reasoning content the provider explicitly returned.
    Reasoning {
        /// Exact bounded text, kept separate from the final answer.
        text: String,
    },
    /// Opaque provider data, such as encrypted reasoning, required for exact stateless replay.
    ProviderReplay(ProviderReplay),
    /// Something the model asked to have run.
    ToolCall(ToolCall),
    /// The answer to one such call. Every recorded call has exactly one of these after it.
    ToolResult {
        /// The call being answered.
        call_id: ToolCallId,
        /// How it ended.
        outcome: ToolOutcome,
    },
}

/// One semantic thing a model produced, in the order it produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelEvent {
    /// Text appended to the message being streamed.
    TextDelta(String),
    /// Plain reasoning text appended separately from the final answer.
    ReasoningDelta(String),
    /// One complete opaque replay item. Incomplete encrypted material is never retained.
    Replay(ProviderReplay),
    /// The model finished asking for one tool call.
    ///
    /// Arrives whole. Accumulating argument fragments across wire deltas and deciding when a call
    /// is complete belongs to the adapter, because how a dialect fragments them is the dialect's.
    Called(ToolCall),
    /// Provider-reported token consumption for this step.
    Usage(TokenUsage),
    /// The model finished this step, and why.
    Stopped(StopReason),
}

/// Why a step ended.
///
/// A typed answer the adapter computes, never a guess the loop makes. A dialect that cannot say
/// why a step ended reports [`StopReason::Unspecified`] rather than letting the loop infer one
/// from whether text arrived, which is how a refusal comes to look like a finished answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    /// The model answered and is not asking for anything.
    EndOfTurn,
    /// The model asked for tools and expects their results.
    ToolCalls,
    /// The model was cut off by the output limit.
    OutputLimit,
    /// The model declined.
    Refused,
    /// The dialect did not say. Treated as the end of the turn, and reported.
    Unspecified,
}

/// A step that did not produce a usable answer.
///
/// Categories rather than messages, because retry, degradation and what the user is told are
/// decisions over this type. An error string used as a machine-readable answer is a defect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelError {
    /// The request never reached the model, or its response never arrived intact.
    Transport {
        /// What to tell the user. Never matched on.
        message: String,
    },
    /// The provider refused the traffic for now.
    RateLimited {
        /// Seconds the provider asked to be left alone, when it said.
        retry_after: Option<u64>,
    },
    /// The conversation no longer fits, so no request can be built from it as it stands.
    ContextTooLong,
    /// The response did not decode into this crate's vocabulary.
    Malformed {
        /// What could not be understood. Never matched on.
        message: String,
    },
}

impl ModelError {
    /// Text for the transcript error a user sees. Presentation, never control flow.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Transport { message } => format!("the model could not be reached: {message}"),
            Self::RateLimited {
                retry_after: Some(seconds),
            } => {
                format!("the provider is rate limiting, retry in {seconds}s")
            }
            Self::RateLimited { retry_after: None } => "the provider is rate limiting".to_owned(),
            Self::ContextTooLong => {
                "the conversation no longer fits the model's context".to_owned()
            }
            Self::Malformed { message } => {
                format!("the model's response was not understood: {message}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PROVIDER_REPLAY_BYTES, ModelError, ProviderCodecId, ProviderReplay,
        ProviderReplayError, StopReason,
    };

    /// `Unspecified` exists so that "the dialect did not say" is a value rather than a guess.
    ///
    /// Deleting it would compile: every caller would fall back to `EndOfTurn`, which is exactly
    /// the silent degradation this variant is here to make visible.
    #[test]
    fn a_dialect_that_cannot_say_why_it_stopped_has_a_value_for_that() {
        assert_ne!(StopReason::Unspecified, StopReason::EndOfTurn);
    }

    #[test]
    fn every_failure_names_itself_without_being_matched_on_as_text() {
        let rate_limited = ModelError::RateLimited {
            retry_after: Some(30),
        };

        assert!(rate_limited.message().contains("30s"));
        assert!(!ModelError::ContextTooLong.message().is_empty());
    }

    /// PRV-3: opaque replay is all-or-nothing, because truncating ciphertext corrupts authority.
    #[test]
    fn provider_replay_is_named_and_bounded_before_turn_state_can_retain_it() {
        assert_eq!(
            ProviderCodecId::new("  "),
            Err(ProviderReplayError::EmptyCodec)
        );
        let codec = ProviderCodecId::new("openai_responses")
            .unwrap_or_else(|error| panic!("fixture codec: {error:?}"));
        let replay = ProviderReplay::new(codec.clone(), "ciphertext".to_owned())
            .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
        let debug = format!("{replay:?}");
        assert!(!debug.contains("ciphertext"));
        assert!(debug.contains("payload_bytes"));
        assert_eq!(
            ProviderReplay::new(codec, "x".repeat(MAX_PROVIDER_REPLAY_BYTES + 1)),
            Err(ProviderReplayError::PayloadTooLarge)
        );
    }
}
