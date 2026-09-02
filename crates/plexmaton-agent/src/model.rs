//! The narrow model surface the loop is written against.
//!
//! These types are the loop's, not a provider's. An adapter translates one dialect's wire form
//! into them and keeps its own vocabulary — response ids, routing metadata, moderation fields — on
//! its own side of the boundary. A shared event type carrying one dialect's concerns leaves every
//! other adapter fabricating fields it does not have.

use plexmaton_core::ToolCallId;

use crate::tools::{ToolCall, ToolOutcome};

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
    /// The model finished asking for one tool call.
    ///
    /// Arrives whole. Accumulating argument fragments across wire deltas and deciding when a call
    /// is complete belongs to the adapter, because how a dialect fragments them is the dialect's.
    Called(ToolCall),
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
    /// Text for the notice a user sees. Presentation, never control flow.
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
    use super::{ModelError, StopReason};

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
}
