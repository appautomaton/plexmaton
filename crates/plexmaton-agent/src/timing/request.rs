//! Validated request-attempt chronology retained outside semantic ancestry.

use std::fmt;

use plexmaton_core::SessionEntryId;
use serde::{Deserialize, Serialize};

use crate::{ModelStepId, UnixMillis};

mod environment;
mod terminal;

pub use environment::{RequestEnvironment, RequestEnvironmentFingerprint};
pub use terminal::{
    DispatchedRequestTiming, ElapsedMillis, RequestAttemptTerminal, RequestAttemptTerminalState,
    RequestCost, RequestDispatchedOutcome, RequestNotDispatchedOutcome, USD_COST_TICKS_PER_DOLLAR,
    UsdCostTicks,
};

const MAX_ATTEMPT_ID_BYTES: usize = 1024;

/// A malformed identity, fingerprint, or monotonic milestone set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestTimingError {
    /// A request-attempt identity contained no non-whitespace text.
    EmptyRequestAttemptId,
    /// A compaction identity contained no non-whitespace text.
    EmptyCompactionId,
    /// A request-attempt or compaction identity exceeded its byte bound.
    IdentityTooLarge,
    /// A serialized environment fingerprint was not 32 lowercase-hex bytes.
    InvalidEnvironmentFingerprint,
    /// A duration could not fit the durable millisecond representation.
    ElapsedMillisOutOfRange,
    /// First output was reported without response headers having arrived.
    FirstOutputWithoutHeaders,
    /// One request milestone preceded the milestone that owns it.
    MilestonesOutOfOrder,
    /// Provider usage contradicted one of its own count or coverage fields.
    InvalidUsage {
        /// Field whose relationship to the rest of the report was invalid.
        field: &'static str,
    },
    /// A known price was retained without its required input and cache categories.
    CostWithoutPricingBreakdown,
}

impl fmt::Display for RequestTimingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyRequestAttemptId => "request-attempt id must not be empty",
            Self::EmptyCompactionId => "compaction id must not be empty",
            Self::IdentityTooLarge => "request timing identity exceeds its byte bound",
            Self::InvalidEnvironmentFingerprint => {
                "request environment fingerprint must be 64 lowercase hexadecimal characters"
            }
            Self::ElapsedMillisOutOfRange => "elapsed duration exceeds its millisecond range",
            Self::FirstOutputWithoutHeaders => {
                "request first output cannot precede response headers"
            }
            Self::MilestonesOutOfOrder => "request timing milestones are out of order",
            Self::InvalidUsage { field } => {
                return write!(formatter, "provider usage field `{field}` is inconsistent");
            }
            Self::CostWithoutPricingBreakdown => {
                "known request cost requires the input cache breakdown"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for RequestTimingError {}

macro_rules! attempt_id {
    ($name:ident, $empty:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Retains one bounded, non-empty external identity.
            pub fn new(value: impl Into<String>) -> Result<Self, RequestTimingError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(RequestTimingError::$empty);
                }
                if value.len() > MAX_ATTEMPT_ID_BYTES {
                    return Err(RequestTimingError::IdentityTooLarge);
                }
                Ok(Self(value))
            }

            /// Stable external representation.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

attempt_id!(
    RequestAttemptId,
    EmptyRequestAttemptId,
    "Stable identity of one authorized model-request attempt."
);
attempt_id!(
    CompactionId,
    EmptyCompactionId,
    "Stable identity of one compaction operation across its model attempts."
);

/// Durable owner of one request attempt without duplicating identities carried by that owner.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestAttemptOwner {
    /// One step whose identity already carries its owning turn.
    AgentStep {
        /// Exact model step being attempted.
        step_id: ModelStepId,
    },
    /// One future compaction operation; its source is the authorization's semantic boundary.
    Compaction {
        /// Stable compaction identity shared by retries of that operation.
        compaction_id: CompactionId,
    },
}

impl RequestAttemptOwner {
    /// Agent step when this attempt contributes to a turn total.
    #[must_use]
    pub const fn agent_step(&self) -> Option<&ModelStepId> {
        match self {
            Self::AgentStep { step_id } => Some(step_id),
            Self::Compaction { .. } => None,
        }
    }
}

/// Durable authorization that must be acknowledged before its request effect starts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestAttemptAuthorized {
    attempt_id: RequestAttemptId,
    owner: RequestAttemptOwner,
    semantic_boundary: SessionEntryId,
    environment: RequestEnvironment,
    authorized_at: UnixMillis,
}

impl RequestAttemptAuthorized {
    /// Binds one attempt to the exact semantic prefix and environment it is allowed to send.
    #[must_use]
    pub const fn new(
        attempt_id: RequestAttemptId,
        owner: RequestAttemptOwner,
        semantic_boundary: SessionEntryId,
        environment: RequestEnvironment,
        authorized_at: UnixMillis,
    ) -> Self {
        Self {
            attempt_id,
            owner,
            semantic_boundary,
            environment,
            authorized_at,
        }
    }

    /// Stable attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> &RequestAttemptId {
        &self.attempt_id
    }

    /// Agent-step or compaction owner.
    #[must_use]
    pub const fn owner(&self) -> &RequestAttemptOwner {
        &self.owner
    }

    /// Last semantic entry included by this request.
    #[must_use]
    pub const fn semantic_boundary(&self) -> &SessionEntryId {
        &self.semantic_boundary
    }

    /// Exact non-context request environment.
    #[must_use]
    pub const fn environment(&self) -> &RequestEnvironment {
        &self.environment
    }

    /// Wall observation sampled before this authorization was appended.
    #[must_use]
    pub const fn authorized_at(&self) -> UnixMillis {
        self.authorized_at
    }
}

/// Derived journal view of one authorization and its optional terminal fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestAttempt {
    authorization: RequestAttemptAuthorized,
    terminal: Option<RequestAttemptTerminal>,
}

#[cfg(test)]
mod tests;

impl RequestAttempt {
    pub(crate) const fn authorized(authorization: RequestAttemptAuthorized) -> Self {
        Self {
            authorization,
            terminal: None,
        }
    }

    pub(crate) fn finish(&mut self, terminal: RequestAttemptTerminal) {
        self.terminal = Some(terminal);
    }

    /// Durable authorization for this attempt.
    #[must_use]
    pub const fn authorization(&self) -> &RequestAttemptAuthorized {
        &self.authorization
    }

    /// Terminal fact, absent when the journal cannot honestly say what occurred after authorization.
    #[must_use]
    pub const fn terminal(&self) -> Option<&RequestAttemptTerminal> {
        self.terminal.as_ref()
    }
}
