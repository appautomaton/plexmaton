use std::time::Duration;

use plexmaton_core::TokenUsage;
use serde::{Deserialize, Serialize};

use super::{RequestAttemptId, RequestTimingError};
use crate::{StopReason, UnixMillis};

/// Fixed-point USD cost precision retained by request terminal facts.
pub const USD_COST_TICKS_PER_DOLLAR: u64 = 10_000_000_000;

/// Non-negative fixed-point USD amount where ten billion ticks equal one dollar.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct UsdCostTicks(u64);

impl UsdCostTicks {
    /// Zero incurred USD cost.
    pub const ZERO: Self = Self(0);

    /// Retains one already-rounded fixed-point USD amount.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Numeric wire value in units of [`USD_COST_TICKS_PER_DOLLAR`].
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Immutable request cost at the prices resolved before dispatch.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "availability", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestCost {
    /// Usage or configured model pricing was insufficient to calculate a cost.
    Unavailable,
    /// Exact fixed-point USD amount calculated for complete provider usage.
    Known {
        /// Non-negative USD ticks at the fixed public precision.
        usd_ticks: UsdCostTicks,
    },
}

/// Milliseconds elapsed on one monotonic request clock.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ElapsedMillis(u64);

impl ElapsedMillis {
    /// Retains an already-checked millisecond offset.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Numeric wire value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<Duration> for ElapsedMillis {
    type Error = RequestTimingError;

    fn try_from(value: Duration) -> Result<Self, Self::Error> {
        u64::try_from(value.as_millis())
            .map(Self)
            .map_err(|_| RequestTimingError::ElapsedMillisOutOfRange)
    }
}

/// Why an attempt ended before `.send()` established the dispatch boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestNotDispatchedOutcome {
    /// The owner cancelled before dispatch.
    Cancelled,
    /// Request preparation failed before encoding.
    PreparationFailed,
    /// The selected codec could not encode the request.
    EncodingFailed,
}

/// Typed outcome of an attempt whose `.send()` boundary was crossed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestDispatchedOutcome {
    /// A valid provider stream reached its typed stop reason.
    Completed {
        /// Provider-declared reason the model step ended.
        stop_reason: StopReason,
    },
    /// The owned request was cancelled after dispatch.
    Cancelled,
    /// Transport failed before a complete response arrived.
    TransportFailed,
    /// The provider refused the request for rate-limit reasons.
    RateLimited,
    /// The provider rejected the context length.
    ContextTooLong,
    /// Provider output could not be decoded without guessing.
    Malformed,
}

/// Validated monotonic milestones from one dispatched request clock.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DispatchedRequestTiming {
    dispatched_at: UnixMillis,
    headers_after_ms: Option<ElapsedMillis>,
    first_output_after_ms: Option<ElapsedMillis>,
    terminal_after_ms: ElapsedMillis,
}

impl DispatchedRequestTiming {
    /// Validates the ordering of every milestone the adapter observed.
    pub fn new(
        dispatched_at: UnixMillis,
        headers_after_ms: Option<ElapsedMillis>,
        first_output_after_ms: Option<ElapsedMillis>,
        terminal_after_ms: ElapsedMillis,
    ) -> Result<Self, RequestTimingError> {
        if first_output_after_ms.is_some() && headers_after_ms.is_none() {
            return Err(RequestTimingError::FirstOutputWithoutHeaders);
        }
        if headers_after_ms.is_some_and(|headers| headers > terminal_after_ms)
            || first_output_after_ms.is_some_and(|first| first > terminal_after_ms)
            || headers_after_ms
                .zip(first_output_after_ms)
                .is_some_and(|(headers, first)| headers > first)
        {
            return Err(RequestTimingError::MilestonesOutOfOrder);
        }
        Ok(Self {
            dispatched_at,
            headers_after_ms,
            first_output_after_ms,
            terminal_after_ms,
        })
    }

    /// Wall time sampled immediately before HTTP dispatch.
    #[must_use]
    pub const fn dispatched_at(&self) -> UnixMillis {
        self.dispatched_at
    }

    /// Time until response headers, when any response arrived.
    #[must_use]
    pub const fn headers_after_ms(&self) -> Option<ElapsedMillis> {
        self.headers_after_ms
    }

    /// Time until the first semantic model output, when one arrived.
    #[must_use]
    pub const fn first_output_after_ms(&self) -> Option<ElapsedMillis> {
        self.first_output_after_ms
    }

    /// Time until the attempt's owned terminal result.
    #[must_use]
    pub const fn terminal_after_ms(&self) -> ElapsedMillis {
        self.terminal_after_ms
    }
}

impl<'de> Deserialize<'de> for DispatchedRequestTiming {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            dispatched_at: UnixMillis,
            headers_after_ms: Option<ElapsedMillis>,
            first_output_after_ms: Option<ElapsedMillis>,
            terminal_after_ms: ElapsedMillis,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.dispatched_at,
            wire.headers_after_ms,
            wire.first_output_after_ms,
            wire.terminal_after_ms,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Valid terminal state of one attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestAttemptTerminalState {
    /// `.send()` was never crossed, so no duration or usage is claimed.
    NotDispatched {
        /// Typed pre-dispatch outcome.
        outcome: RequestNotDispatchedOutcome,
    },
    /// `.send()` was crossed and all retained measurements share one request clock.
    Dispatched {
        /// Validated wall and monotonic milestones.
        timing: DispatchedRequestTiming,
        /// Typed provider/transport outcome.
        outcome: RequestDispatchedOutcome,
        /// Exact provider-reported usage, or explicit unavailability.
        usage: TokenUsage,
        /// Cost fixed under the model pricing resolved for this attempt.
        cost: RequestCost,
    },
}

/// One immutable terminal fact correlated only through its attempt identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestAttemptTerminal {
    attempt_id: RequestAttemptId,
    terminal: RequestAttemptTerminalState,
}

impl RequestAttemptTerminal {
    /// Ends one attempt with a pre-dispatch or dispatched state.
    pub fn new(
        attempt_id: RequestAttemptId,
        terminal: RequestAttemptTerminalState,
    ) -> Result<Self, RequestTimingError> {
        validate_terminal(&terminal)?;
        Ok(Self {
            attempt_id,
            terminal,
        })
    }

    /// Attempt this terminal fact settles.
    #[must_use]
    pub const fn attempt_id(&self) -> &RequestAttemptId {
        &self.attempt_id
    }

    /// Honest terminal state and its measurements.
    #[must_use]
    pub const fn terminal(&self) -> &RequestAttemptTerminalState {
        &self.terminal
    }

    /// Cost this attempt incurred without consulting current model configuration.
    #[must_use]
    pub const fn incurred_cost(&self) -> RequestCost {
        match &self.terminal {
            RequestAttemptTerminalState::NotDispatched { .. } => RequestCost::Known {
                usd_ticks: UsdCostTicks::ZERO,
            },
            RequestAttemptTerminalState::Dispatched { cost, .. } => *cost,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), RequestTimingError> {
        validate_terminal(&self.terminal)
    }
}

impl<'de> Deserialize<'de> for RequestAttemptTerminal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            attempt_id: RequestAttemptId,
            terminal: RequestAttemptTerminalState,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.attempt_id, wire.terminal).map_err(serde::de::Error::custom)
    }
}

fn validate_terminal(terminal: &RequestAttemptTerminalState) -> Result<(), RequestTimingError> {
    let RequestAttemptTerminalState::Dispatched { usage, cost, .. } = terminal else {
        return Ok(());
    };
    if matches!(cost, RequestCost::Known { .. }) && !matches!(usage, TokenUsage::Complete(_)) {
        return Err(RequestTimingError::CostWithoutCompleteUsage);
    }
    let Some(counts) = usage.counts() else {
        return Ok(());
    };
    if counts
        .cached_input
        .is_some_and(|value| value > counts.input)
    {
        return Err(RequestTimingError::InvalidUsage {
            field: "cached_input",
        });
    }
    if counts
        .cache_write_input
        .is_some_and(|value| value > counts.input)
    {
        return Err(RequestTimingError::InvalidUsage {
            field: "cache_write_input",
        });
    }
    if counts
        .cached_input
        .zip(counts.cache_write_input)
        .is_some_and(|(cached, written)| {
            cached
                .checked_add(written)
                .is_none_or(|combined| combined > counts.input)
        })
    {
        return Err(RequestTimingError::InvalidUsage {
            field: "input_breakdown",
        });
    }
    if counts
        .reasoning_output
        .is_some_and(|value| value > counts.output)
    {
        return Err(RequestTimingError::InvalidUsage {
            field: "reasoning_output",
        });
    }
    if counts.input.checked_add(counts.output) != Some(counts.total) {
        return Err(RequestTimingError::InvalidUsage { field: "total" });
    }
    if matches!(usage, TokenUsage::Complete(_))
        && (counts.cached_input.is_none()
            || counts.cache_write_input.is_none()
            || counts.reasoning_output.is_none())
    {
        return Err(RequestTimingError::InvalidUsage { field: "coverage" });
    }
    Ok(())
}
