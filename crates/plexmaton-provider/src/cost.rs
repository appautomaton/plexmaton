//! Immutable request cost calculated from one resolved price table and its required usage categories.

use plexmaton_agent::{RequestCost, USD_COST_TICKS_PER_DOLLAR, UsdCostTicks};
use plexmaton_core::{TokenCounts, TokenUsage};

use crate::{ModelCost, ResolvedModel};

const TOKENS_PER_MILLION: u128 = 1_000_000;

/// Calculates the fixed cost from the final usage of one completed request.
///
/// The caller must establish protocol completion; field coverage does not establish finality.
/// Interim snapshots from cancelled/failed attempts and aggregate usage are not eligible.
/// Missing pricing or required cache categories, inconsistent subsets, or an unrepresentable amount stays
/// unavailable. The returned amount is rounded once to the nearest public USD tick.
#[must_use]
pub fn request_cost(model: &ResolvedModel, usage: &TokenUsage) -> RequestCost {
    let Some(prices) = model.cost() else {
        return RequestCost::Unavailable;
    };
    let Some(counts) = usage.counts() else {
        return RequestCost::Unavailable;
    };
    calculate(prices, counts).map_or(RequestCost::Unavailable, |usd_ticks| RequestCost::Known {
        usd_ticks: UsdCostTicks::new(usd_ticks),
    })
}

fn calculate(prices: &ModelCost, counts: &TokenCounts) -> Option<u64> {
    if counts.input.checked_add(counts.output)? != counts.total
        || counts
            .reasoning_output
            .is_some_and(|reasoning| reasoning > counts.output)
    {
        return None;
    }
    let cached = counts.cached_input?;
    let cache_write = counts.cache_write_input?;
    let priced_input = cached.checked_add(cache_write)?;
    let uncached = counts.input.checked_sub(priced_input)?;
    let components = [
        (uncached, prices.input()),
        (cached, prices.cache_read()),
        (cache_write, prices.cache_write()),
        (counts.output, prices.output()),
    ];
    let mut numerator = 0_u128;
    for (tokens, usd_per_million) in components {
        let price_ticks = price_ticks_per_million(usd_per_million)?;
        numerator = numerator.checked_add(u128::from(tokens).checked_mul(price_ticks)?)?;
    }
    let rounded = numerator
        .checked_add(TOKENS_PER_MILLION / 2)?
        .checked_div(TOKENS_PER_MILLION)?;
    u64::try_from(rounded).ok()
}

fn price_ticks_per_million(usd_per_million: f64) -> Option<u128> {
    let scaled = usd_per_million * USD_COST_TICKS_PER_DOLLAR as f64;
    if !scaled.is_finite() || scaled < 0.0 || scaled > u64::MAX as f64 {
        return None;
    }
    Some(u128::from(scaled.round() as u64))
}

#[cfg(test)]
mod tests;
