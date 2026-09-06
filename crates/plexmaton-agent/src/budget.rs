//! Pure occupancy arithmetic shared by request planning and diagnostics.

use plexmaton_core::ConversationEntryId;
use serde::{Deserialize, Serialize};

use crate::{RequestAttemptId, RequestEnvironment};

#[cfg(test)]
mod tests;

/// Versioned algorithm for context the provider has not measured.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenEstimator {
    #[default]
    Utf8HeuristicV1,
}

impl TokenEstimator {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8HeuristicV1 => "utf8_heuristic_v1",
        }
    }
}

/// A provider's input count for one exact, environment-inclusive atom prefix.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InputUsageAnchor {
    pub(crate) attempt_id: RequestAttemptId,
    pub(crate) atom_count: usize,
    pub(crate) input_tokens: u64,
}

impl InputUsageAnchor {
    #[must_use]
    pub const fn attempt_id(&self) -> &RequestAttemptId {
        &self.attempt_id
    }
    #[must_use]
    pub const fn atom_count(&self) -> usize {
        self.atom_count
    }
    #[must_use]
    pub const fn input_tokens(&self) -> u64 {
        self.input_tokens
    }
}

/// Byte-based approximation, including the opaque subset whose token count is unknowable locally.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct TokenEstimate {
    pub tokens: u64,
    pub opaque_replay_bytes: u64,
}

impl TokenEstimate {
    pub fn checked_add(self, other: Self) -> Result<Self, BudgetError> {
        Ok(Self {
            tokens: add(self.tokens, other.tokens)?,
            opaque_replay_bytes: add(self.opaque_replay_bytes, other.opaque_replay_bytes)?,
        })
    }
}

/// One indivisible atom's estimate, with identities usable by the compaction planner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AtomBudget {
    pub source_entries: Box<[ConversationEntryId]>,
    pub estimate: TokenEstimate,
}

/// Validated policy for one request; no wire options or mutable model configuration live here.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BudgetLimits {
    context_window_tokens: u64,
    output_reserve_tokens: u64,
    soft_input_limit: u64,
}

impl BudgetLimits {
    pub fn new(window: u64, reserve: u64, soft: u64) -> Result<Self, BudgetError> {
        if window == 0 || reserve >= window || soft == 0 || soft > window - reserve {
            return Err(BudgetError::InvalidLimits);
        }
        Ok(Self {
            context_window_tokens: window,
            output_reserve_tokens: reserve,
            soft_input_limit: soft,
        })
    }

    #[must_use]
    pub const fn context_window_tokens(self) -> u64 {
        self.context_window_tokens
    }
    #[must_use]
    pub const fn output_reserve_tokens(self) -> u64 {
        self.output_reserve_tokens
    }
    #[must_use]
    pub const fn soft_input_limit(self) -> u64 {
        self.soft_input_limit
    }
    #[must_use]
    pub const fn input_capacity(self) -> u64 {
        self.context_window_tokens - self.output_reserve_tokens
    }
}

/// A fit assessment over the ledger's explicitly measured/estimated occupancy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BudgetDecision {
    Fits,
    CompactionNeeded { pressure: BudgetPressure },
    ImpossibleItem { item: OversizedInput },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetPressure {
    SoftLimit,
    HardLimit,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OversizedInput {
    Environment,
    Atom { index: usize },
}

/// Redaction-safe shared snapshot; measured prefix and estimated remainder are never conflated.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BudgetLedger {
    pub environment: RequestEnvironment,
    pub estimator: TokenEstimator,
    pub limits: BudgetLimits,
    pub environment_estimate: TokenEstimate,
    pub atoms: Vec<AtomBudget>,
    pub anchor: Option<InputUsageAnchor>,
    /// Includes the environment only when no provider anchor covers it.
    pub estimated_remainder: TokenEstimate,
    pub input_tokens: u64,
    pub remaining_input_tokens: u64,
    pub decision: BudgetDecision,
}

impl BudgetLedger {
    pub fn from_estimates(
        environment: RequestEnvironment,
        estimator: TokenEstimator,
        limits: BudgetLimits,
        environment_estimate: TokenEstimate,
        atoms: Vec<AtomBudget>,
        anchor: Option<InputUsageAnchor>,
    ) -> Result<Self, BudgetError> {
        let prefix = anchor.as_ref().map_or(0, |anchor| anchor.atom_count);
        if prefix > atoms.len() {
            return Err(BudgetError::InvalidAnchor);
        }
        let estimated_remainder = atoms[prefix..].iter().try_fold(
            if anchor.is_some() {
                TokenEstimate::default()
            } else {
                environment_estimate
            },
            |total, atom| total.checked_add(atom.estimate),
        )?;
        let input_tokens = add(
            anchor.as_ref().map_or(0, |anchor| anchor.input_tokens),
            estimated_remainder.tokens,
        )?;
        let decision = decide(
            limits,
            // The environment is part of the measured prefix, so its true cost cannot
            // exceed that whole measurement even when our byte heuristic overshoots.
            anchor
                .as_ref()
                .map_or(environment_estimate.tokens, |anchor| {
                    environment_estimate.tokens.min(anchor.input_tokens)
                }),
            &atoms,
            prefix,
            anchor.is_some(),
            input_tokens,
        )?;
        Ok(Self {
            environment,
            estimator,
            limits,
            environment_estimate,
            atoms,
            anchor,
            estimated_remainder,
            input_tokens,
            remaining_input_tokens: limits.input_capacity().saturating_sub(input_tokens),
            decision,
        })
    }
}

fn decide(
    limits: BudgetLimits,
    environment: u64,
    atoms: &[AtomBudget],
    measured_prefix: usize,
    anchored: bool,
    input: u64,
) -> Result<BudgetDecision, BudgetError> {
    // A measured fit outranks a larger heuristic for the same covered bytes.
    if input <= limits.soft_input_limit {
        return Ok(BudgetDecision::Fits);
    }
    if !anchored && environment > limits.input_capacity() {
        return Ok(BudgetDecision::ImpossibleItem {
            item: OversizedInput::Environment,
        });
    }
    for (index, atom) in atoms.iter().enumerate().skip(measured_prefix) {
        if add(environment, atom.estimate.tokens)? > limits.input_capacity() {
            return Ok(BudgetDecision::ImpossibleItem {
                item: OversizedInput::Atom { index },
            });
        }
    }
    Ok(BudgetDecision::CompactionNeeded {
        pressure: if input > limits.input_capacity() {
            BudgetPressure::HardLimit
        } else {
            BudgetPressure::SoftLimit
        },
    })
}

fn add(left: u64, right: u64) -> Result<u64, BudgetError> {
    left.checked_add(right).ok_or(BudgetError::Overflow)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetError {
    InvalidLimits,
    InvalidAnchor,
    Overflow,
}

impl std::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidLimits => "invalid context budget limits",
            Self::InvalidAnchor => "usage anchor is outside the atom prefix",
            Self::Overflow => "context budget arithmetic overflowed",
        })
    }
}
impl std::error::Error for BudgetError {}
