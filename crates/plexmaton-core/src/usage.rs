//! Provider-reported token counts shared by runtime and projection.

use serde::{Deserialize, Serialize};

/// Token counts reported by a provider for one step or aggregated for one turn.
///
/// The provider's total is retained instead of recomputed. Optional breakdowns remain optional so
/// a dialect that omits one cannot turn absence into zero (LIVE-4, LIVE-5).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TokenCounts {
    /// Tokens in the request context, including any cached or cache-written subset.
    pub input: u64,
    /// Input tokens served from a provider cache, when reported.
    pub cached_input: Option<u64>,
    /// Input tokens written to a provider cache, when reported.
    pub cache_write_input: Option<u64>,
    /// Generated tokens, including any reasoning subset.
    pub output: u64,
    /// Generated reasoning tokens, when reported separately.
    pub reasoning_output: Option<u64>,
    /// Provider-reported total. Consumers must not reconstruct this field.
    pub total: u64,
}

impl TokenCounts {
    /// Adds reported counts without wrapping and retains every known partial breakdown.
    #[must_use]
    pub fn checked_add(&self, other: &Self) -> Option<Self> {
        Some(Self {
            input: self.input.checked_add(other.input)?,
            cached_input: checked_add_known(self.cached_input, other.cached_input)?,
            cache_write_input: checked_add_known(self.cache_write_input, other.cache_write_input)?,
            output: self.output.checked_add(other.output)?,
            reasoning_output: checked_add_known(self.reasoning_output, other.reasoning_output)?,
            total: self.total.checked_add(other.total)?,
        })
    }
}

fn checked_add_known(left: Option<u64>, right: Option<u64>) -> Option<Option<u64>> {
    match (left, right) {
        (Some(left), Some(right)) => Some(Some(left.checked_add(right)?)),
        (Some(value), None) | (None, Some(value)) => Some(Some(value)),
        (None, None) => Some(None),
    }
}

/// How much of a step or turn's provider usage is known.
///
/// Counts cannot accompany `Unavailable`, and `Partial` cannot be mistaken for a complete bill.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "coverage", content = "counts", rename_all = "snake_case")]
pub enum TokenUsage {
    /// Every count the selected dialect promises was reported.
    Complete(TokenCounts),
    /// Some reported counts are exact, while at least one step or breakdown is absent.
    Partial(TokenCounts),
    /// The provider supplied no usable counts.
    Unavailable,
}

impl TokenUsage {
    /// Exact counts known for this usage report, if any.
    #[must_use]
    pub const fn counts(&self) -> Option<&TokenCounts> {
        match self {
            Self::Complete(counts) | Self::Partial(counts) => Some(counts),
            Self::Unavailable => None,
        }
    }
}
