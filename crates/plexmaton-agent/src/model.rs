//! The narrow model surface the loop is written against.
//!
//! These types are the loop's, not a provider's. An adapter translates one dialect's wire form
//! into them. Replay compatibility crosses this boundary because canonical context must decide
//! whether opaque state can be reused; response ids, transport routing and moderation fields stay
//! adapter-side. A shared event type carrying one dialect's concerns leaves every other adapter
//! fabricating fields it does not have.

use plexmaton_core::{TokenUsage, TurnId};
use serde::{Deserialize, Serialize};

use crate::tools::ToolCall;

mod context;
mod replay;

pub use context::{
    AssistantBlock, AssistantOutput, AssistantReplay, BlockReplay, ContextAtom, ContextAtomValue,
    ContextError, MAX_ASSISTANT_TEXT_BYTES, MAX_ASSISTANT_TOOL_ARGUMENT_BYTES,
    MAX_TOOL_IDENTITY_BYTES, ModelOutputPosition, ToolBatch, ToolBatchResult,
};
pub use replay::{
    MAX_PROVIDER_REPLAY_BYTES, ProviderCodecId, ProviderCodecRevision, ProviderModelFamilyId,
    ProviderReplay, ProviderReplayError, ProviderReplayOwnerId, ReplayCompatibility,
};

/// Stable identity of one model request within a turn (LIVE-2).
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ModelStepId {
    turn_id: TurnId,
    index: u16,
}

impl<'de> Deserialize<'de> for ModelStepId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            turn_id: TurnId,
            index: u16,
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.index == 0 {
            return Err(serde::de::Error::custom(
                "model step index must be one-based",
            ));
        }
        Ok(Self::new(wire.turn_id, wire.index))
    }
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
    /// Indivisible conversation units, oldest first.
    pub atoms: Vec<ContextAtom>,
}

/// One correlated request the runtime must perform.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCall {
    /// Stable identity every resulting event or failure must echo.
    pub step_id: ModelStepId,
    /// Stateless semantic conversation to encode.
    pub request: ModelRequest,
}

/// One semantic thing a model produced, in the order it produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelEvent {
    /// Text appended to the message being streamed.
    TextDelta {
        position: ModelOutputPosition,
        delta: String,
    },
    /// Plain reasoning text appended separately from the final answer.
    ReasoningDelta {
        position: ModelOutputPosition,
        delta: String,
    },
    /// One complete opaque replay item. Incomplete encrypted material is never retained.
    Replay {
        position: ModelOutputPosition,
        replay: ProviderReplay,
    },
    /// The model finished asking for one tool call.
    ///
    /// Arrives whole. Accumulating argument fragments across wire deltas and deciding when a call
    /// is complete belongs to the adapter, because how a dialect fragments them is the dialect's.
    Called {
        position: ModelOutputPosition,
        call: ToolCall,
    },
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
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
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
    use crate::test_support::replay_compatibility;

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
        let compatibility = replay_compatibility();
        assert_eq!(
            ProviderReplay::new(compatibility.clone(), String::new()),
            Err(ProviderReplayError::EmptyPayload)
        );
        let replay = ProviderReplay::new(compatibility.clone(), "ciphertext".to_owned())
            .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
        let debug = format!("{replay:?}");
        assert!(!debug.contains("ciphertext"));
        assert!(debug.contains("payload_bytes"));
        assert_eq!(
            ProviderReplay::new(compatibility, "x".repeat(MAX_PROVIDER_REPLAY_BYTES + 1),),
            Err(ProviderReplayError::PayloadTooLarge)
        );
    }
}
