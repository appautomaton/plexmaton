//! Checked provider-reported usage retained only while its turn is open.

use plexmaton_core::{TokenCounts, TokenUsage};

/// Aggregate of exact step reports for one turn (LIVE-4, LIVE-5).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct UsageAccumulator {
    counts: Option<TokenCounts>,
    partial: bool,
    unavailable: bool,
}

impl UsageAccumulator {
    /// Adds one step without saturating or reconstructing the provider's total.
    pub(super) fn add(&mut self, report: TokenUsage) -> Result<TokenUsage, ()> {
        match report {
            TokenUsage::Complete(counts) => self.add_counts(counts)?,
            TokenUsage::Partial(counts) => {
                self.partial = true;
                self.add_counts(counts)?;
            }
            TokenUsage::Unavailable => self.unavailable = true,
        }
        Ok(self.snapshot())
    }

    fn add_counts(&mut self, counts: TokenCounts) -> Result<(), ()> {
        let combined = match self.counts.as_ref() {
            Some(existing) => existing.checked_add(&counts).ok_or(())?,
            None => counts,
        };
        self.counts = Some(combined);
        Ok(())
    }

    fn snapshot(&self) -> TokenUsage {
        let Some(counts) = self.counts.clone() else {
            return TokenUsage::Unavailable;
        };
        if self.partial || self.unavailable {
            TokenUsage::Partial(counts)
        } else {
            TokenUsage::Complete(counts)
        }
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{TokenCounts, TokenUsage};

    use super::UsageAccumulator;

    fn counts(input: u64, output: u64, total: u64) -> TokenCounts {
        TokenCounts {
            input,
            cached_input: Some(2),
            cache_write_input: Some(1),
            output,
            reasoning_output: Some(3),
            total,
        }
    }

    /// LIVE-4: every provider total is added as reported rather than reconstructed from subsets.
    #[test]
    fn complete_step_reports_are_checked_and_aggregated_for_the_turn() {
        let mut usage = UsageAccumulator::default();
        let _first = usage
            .add(TokenUsage::Complete(counts(10, 7, 19)))
            .expect("first report fits");
        let aggregate = usage
            .add(TokenUsage::Complete(counts(12, 9, 25)))
            .expect("turn aggregate fits");

        let TokenUsage::Complete(aggregate) = aggregate else {
            panic!("two complete reports stay complete");
        };
        assert_eq!(aggregate.input, 22);
        assert_eq!(aggregate.output, 16);
        assert_eq!(aggregate.total, 44, "the provider totals are not rebuilt");
        assert_eq!(aggregate.cached_input, Some(4));
    }

    /// LIVE-5: one missing step makes known totals partial, while all-missing stays unavailable.
    #[test]
    fn missing_step_usage_is_never_presented_as_zero() {
        let mut all_missing = UsageAccumulator::default();
        assert_eq!(
            all_missing.add(TokenUsage::Unavailable),
            Ok(TokenUsage::Unavailable)
        );

        let mut partial = UsageAccumulator::default();
        let _known = partial
            .add(TokenUsage::Complete(counts(10, 7, 17)))
            .expect("known report");
        assert!(matches!(
            partial.add(TokenUsage::Unavailable),
            Ok(TokenUsage::Partial(_))
        ));
    }

    #[test]
    fn an_overflow_never_wraps_or_saturates_a_turn_total() {
        let mut usage = UsageAccumulator::default();
        let _first = usage
            .add(TokenUsage::Complete(counts(u64::MAX, 1, 1)))
            .expect("first report fits without addition");
        assert_eq!(usage.add(TokenUsage::Complete(counts(1, 1, 1))), Err(()));
    }
}
