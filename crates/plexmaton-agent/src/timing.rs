//! Durable chronology values observed outside the pure agent reducer.

use plexmaton_core::{AgentId, AgentStatus, ConversationEntryId, TurnId};
use serde::{Deserialize, Serialize};

mod request;
mod usage;

pub(crate) use usage::UsageAccumulator;

pub use request::{
    CompactionId, DispatchedRequestTiming, ElapsedMillis, RequestAttempt, RequestAttemptAuthorized,
    RequestAttemptId, RequestAttemptOwner, RequestAttemptTerminal, RequestAttemptTerminalState,
    RequestCost, RequestDispatchedOutcome, RequestEnvironment, RequestEnvironmentFingerprint,
    RequestNotDispatchedOutcome, RequestTimingError, USD_COST_TICKS_PER_DOLLAR, UsdCostTicks,
};

/// Milliseconds since the Unix epoch, used only to place facts on a wall-clock chronology.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct UnixMillis(u64);

impl UnixMillis {
    /// The Unix epoch, useful as a deterministic synthetic clock value.
    pub const EPOCH: Self = Self(0);

    /// Retains one externally observed wall-clock value.
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

/// A visible status change within one already-open turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveTurnStatus {
    /// The agent is producing or preparing a model step.
    Running,
    /// The agent is waiting for owned tool or approval work.
    Waiting,
}

impl ActiveTurnStatus {
    /// Existing UI vocabulary projected from this scoped durable state.
    #[must_use]
    pub const fn agent_status(self) -> AgentStatus {
        match self {
            Self::Running => AgentStatus::Running,
            Self::Waiting => AgentStatus::Waiting,
        }
    }
}

/// Why one turn stopped owning live work.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    /// The model stopped without leaving unpaid work.
    Completed,
    /// The user interrupted the active turn.
    Interrupted,
    /// A typed model or loop failure ended the turn.
    Failed,
    /// The runtime began orderly shutdown.
    Shutdown,
    /// The turn exhausted its bounded model-step budget.
    StepBudgetReached,
    /// The prior process disappeared while the turn was open.
    ProcessDied,
}

/// Which wall observation is honestly available at one terminal boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnFinishedAt {
    /// The live owner observed the terminal transition.
    Observed {
        /// Wall time sampled for the completed transition.
        completed_at: UnixMillis,
    },
    /// A later process observed that the prior owner had disappeared.
    Recovered {
        /// Wall time at recovery, not a fabricated completion time.
        recovery_observed_at: UnixMillis,
    },
}

/// One immutable, non-head-advancing terminal turn fact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TurnFinished {
    /// Agent whose turn ended.
    pub agent_id: AgentId,
    /// Stable turn identity introduced by its semantic start entry.
    pub turn_id: TurnId,
    /// Last semantic entry owned by the turn on its original branch.
    pub semantic_boundary: ConversationEntryId,
    /// Typed terminal outcome.
    pub outcome: TurnOutcome,
    /// Wall observation available for that outcome.
    pub at: TurnFinishedAt,
}
