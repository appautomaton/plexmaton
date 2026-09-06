use super::*;
use crate::{RequestEnvironmentFingerprint, test_support::replay_compatibility};

fn environment() -> RequestEnvironment {
    RequestEnvironment::new(
        replay_compatibility(),
        RequestEnvironmentFingerprint::new([1; 32]),
    )
}
fn atom(tokens: u64) -> AtomBudget {
    AtomBudget {
        source_entries: vec![SessionEntryId::new("entry").expect("fixture id")].into(),
        estimate: TokenEstimate {
            tokens,
            opaque_replay_bytes: 0,
        },
    }
}
fn ledger(
    environment_tokens: u64,
    tokens: &[u64],
    anchor: Option<InputUsageAnchor>,
) -> Result<BudgetLedger, BudgetError> {
    BudgetLedger::from_estimates(
        environment(),
        TokenEstimator::default(),
        BudgetLimits::new(100, 20, 60).expect("limits"),
        TokenEstimate {
            tokens: environment_tokens,
            opaque_replay_bytes: 0,
        },
        tokens.iter().copied().map(atom).collect(),
        anchor,
    )
}
fn anchor(count: usize, input: u64) -> InputUsageAnchor {
    InputUsageAnchor {
        attempt_id: RequestAttemptId::new("attempt").expect("id"),
        context_epoch: ContextEpoch::Original,
        atom_count: count,
        input_tokens: input,
    }
}

/// BUD-4: equality fits; reserve, soft pressure, hard pressure and indivisible items are distinct.
#[test]
fn bud_4_decisions_cover_soft_hard_reserve_and_indivisible_boundaries() {
    for (tokens, expected) in [
        (vec![50], BudgetDecision::Fits),
        (
            vec![51],
            BudgetDecision::CompactionNeeded {
                pressure: BudgetPressure::SoftLimit,
            },
        ),
        (
            vec![35, 35],
            BudgetDecision::CompactionNeeded {
                pressure: BudgetPressure::SoftLimit,
            },
        ),
        (
            vec![35, 36],
            BudgetDecision::CompactionNeeded {
                pressure: BudgetPressure::HardLimit,
            },
        ),
        (
            vec![71],
            BudgetDecision::ImpossibleItem {
                item: OversizedInput::Atom { index: 0 },
            },
        ),
    ] {
        let result = ledger(10, &tokens, None).expect("ledger");
        assert_eq!(result.decision, expected);
        assert_eq!(result.limits.input_capacity(), 80);
        assert_eq!(
            result.remaining_input_tokens,
            80_u64.saturating_sub(10 + tokens.iter().sum::<u64>())
        );
    }
    assert_eq!(
        ledger(81, &[], None).expect("ledger").decision,
        BudgetDecision::ImpossibleItem {
            item: OversizedInput::Environment
        }
    );
}

/// BUD-2/BUD-4: measured occupancy replaces its heuristic, and environment is included once.
#[test]
fn bud_2_measured_prefix_replaces_estimates_without_double_counting_environment() {
    let result = ledger(90, &[200, 5], Some(anchor(1, 20))).expect("ledger");
    assert_eq!(result.input_tokens, 25);
    assert_eq!(result.estimated_remainder.tokens, 5);
    assert_eq!(result.decision, BudgetDecision::Fits);
    let exact = ledger(90, &[200], Some(anchor(1, 20))).expect("ledger");
    assert_eq!(exact.estimated_remainder, TokenEstimate::default());
    assert_eq!(exact.input_tokens, 20);
    let pressured = ledger(90, &[200, 45], Some(anchor(1, 20))).expect("ledger");
    assert_eq!(pressured.input_tokens, 65);
    assert_eq!(
        pressured.decision,
        BudgetDecision::CompactionNeeded {
            pressure: BudgetPressure::SoftLimit
        }
    );
}

/// BUD-3/BUD-4: absence is explicit, not an exact zero, and opaque uncertainty survives sums.
#[test]
fn bud_3_unmeasured_and_opaque_inputs_keep_their_estimate_provenance() {
    let result = ledger(10, &[5], None).expect("ledger");
    assert!(result.anchor.is_none());
    assert_eq!(result.estimated_remainder.tokens, 15);
    assert_eq!(
        TokenEstimate {
            tokens: 8,
            opaque_replay_bytes: 30
        }
        .checked_add(TokenEstimate {
            tokens: 2,
            opaque_replay_bytes: 0
        }),
        Ok(TokenEstimate {
            tokens: 10,
            opaque_replay_bytes: 30
        })
    );
}

/// BUD-4: malformed limits, prefix boundaries and arithmetic fail without producing a ledger.
#[test]
fn bud_4_invalid_limits_anchors_and_overflow_are_typed() {
    for (window, reserve, soft) in [(0, 0, 1), (100, 100, 1), (100, 20, 81), (100, 20, 0)] {
        assert_eq!(
            BudgetLimits::new(window, reserve, soft),
            Err(BudgetError::InvalidLimits)
        );
    }
    assert_eq!(
        ledger(1, &[1], Some(anchor(2, 1))),
        Err(BudgetError::InvalidAnchor)
    );
    assert_eq!(ledger(u64::MAX, &[1], None), Err(BudgetError::Overflow));
    assert_eq!(
        ledger(1, &[1, 1], Some(anchor(1, u64::MAX))),
        Err(BudgetError::Overflow)
    );
}
