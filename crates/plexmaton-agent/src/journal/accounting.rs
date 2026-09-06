//! On-demand incurred accounting over immutable request attempts (TIM-3).

use std::fmt;

use plexmaton_core::TokenUsage;

use super::ConversationJournal;
use crate::timing::UsageAccumulator;
use crate::{
    RequestAttempt, RequestAttemptId, RequestAttemptOwner, RequestAttemptTerminalState,
    RequestCost, UsdCostTicks,
};

/// Provider-reported usage and immutable incurred cost for a set of request attempts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestAccounting {
    /// Reported counts, partial when any dispatched or unresolved attempt lacks complete usage.
    /// With no provider reports this remains unavailable, including when nothing was dispatched.
    pub usage: TokenUsage,
    /// Known only when every attempt's incurred cost is known. An empty set incurs zero.
    pub cost: RequestCost,
}

/// A cumulative total exceeded its integer representation; no partial result is returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestAccountingError {
    /// At least one provider-reported token count could not be added without overflow.
    UsageOverflow {
        /// Attempt whose addition exceeded the representable total.
        attempt_id: RequestAttemptId,
    },
    /// Known USD ticks could not be added without overflow.
    CostOverflow {
        /// Attempt whose addition exceeded the representable total.
        attempt_id: RequestAttemptId,
    },
}

impl fmt::Display for RequestAccountingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UsageOverflow { attempt_id } => {
                write!(
                    formatter,
                    "request usage total overflowed at attempt {attempt_id}"
                )
            }
            Self::CostOverflow { attempt_id } => {
                write!(
                    formatter,
                    "request cost total overflowed at attempt {attempt_id}"
                )
            }
        }
    }
}

impl std::error::Error for RequestAccountingError {}

impl ConversationJournal {
    /// Incurred facts for one stable turn identity, excluding compaction and every other turn.
    pub fn turn_accounting(
        &self,
        turn: &plexmaton_core::TurnId,
    ) -> Result<RequestAccounting, RequestAccountingError> {
        fold_attempts(self.request_attempts().filter(|attempt| {
            attempt
                .authorization()
                .owner()
                .agent_step()
                .is_some_and(|step| step.turn_id() == turn)
        }))
    }

    /// Folds every unique authorized attempt once, including compaction and abandoned branches.
    /// An authorization without a terminal fact contributes unavailable usage and cost (TIM-5).
    pub fn incurred_accounting(&self) -> Result<RequestAccounting, RequestAccountingError> {
        fold_attempts(self.request_attempts())
    }

    /// Folds only compaction attempts, across every branch and compaction retry.
    pub fn compaction_accounting(&self) -> Result<RequestAccounting, RequestAccountingError> {
        fold_attempts(self.request_attempts().filter(|attempt| {
            matches!(
                attempt.authorization().owner(),
                RequestAttemptOwner::Compaction { .. }
            )
        }))
    }
}

fn fold_attempts<'a>(
    attempts: impl IntoIterator<Item = &'a RequestAttempt>,
) -> Result<RequestAccounting, RequestAccountingError> {
    let mut accumulator = UsageAccumulator::default();
    let mut usage = TokenUsage::Unavailable;
    let mut known_cost_ticks = 0_u64;
    let mut cost_unavailable = false;
    for attempt in attempts {
        let terminal = attempt.terminal();
        let report = match terminal.map(crate::RequestAttemptTerminal::terminal) {
            Some(RequestAttemptTerminalState::NotDispatched { .. }) => continue,
            Some(RequestAttemptTerminalState::Dispatched { usage, .. }) => usage.clone(),
            None => TokenUsage::Unavailable,
        };
        usage = accumulator
            .add(report)
            .map_err(|()| RequestAccountingError::UsageOverflow {
                attempt_id: attempt.authorization().attempt_id().clone(),
            })?;
        match terminal.map_or(RequestCost::Unavailable, |terminal| {
            terminal.incurred_cost()
        }) {
            RequestCost::Unavailable => cost_unavailable = true,
            RequestCost::Known { usd_ticks } => {
                known_cost_ticks =
                    known_cost_ticks
                        .checked_add(usd_ticks.get())
                        .ok_or_else(|| RequestAccountingError::CostOverflow {
                            attempt_id: attempt.authorization().attempt_id().clone(),
                        })?;
            }
        }
    }
    let cost = if cost_unavailable {
        RequestCost::Unavailable
    } else {
        RequestCost::Known {
            usd_ticks: UsdCostTicks::new(known_cost_ticks),
        }
    };
    Ok(RequestAccounting { usage, cost })
}
