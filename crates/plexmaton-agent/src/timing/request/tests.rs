use std::time::Duration;

use plexmaton_core::{SessionEntryId, TokenCounts, TokenUsage, TurnId};

use super::{
    CompactionId, DispatchedRequestTiming, ElapsedMillis, RequestAttemptAuthorized,
    RequestAttemptId, RequestAttemptOwner, RequestAttemptTerminal, RequestAttemptTerminalState,
    RequestDispatchedOutcome, RequestEnvironment, RequestEnvironmentFingerprint,
    RequestNotDispatchedOutcome, RequestTimingError,
};
use crate::test_support::replay_compatibility;
use crate::{ModelStepId, StopReason, UnixMillis};

fn attempt(value: &str) -> RequestAttemptId {
    RequestAttemptId::new(value).unwrap_or_else(|error| panic!("attempt fixture: {error}"))
}

fn compaction(value: &str) -> CompactionId {
    CompactionId::new(value).unwrap_or_else(|error| panic!("compaction fixture: {error}"))
}

fn entry(value: &str) -> SessionEntryId {
    SessionEntryId::new(value).unwrap_or_else(|error| panic!("entry fixture: {error}"))
}

fn step(turn: &str, index: u16) -> ModelStepId {
    ModelStepId::new(
        TurnId::new(turn).unwrap_or_else(|error| panic!("turn fixture: {error}")),
        index,
    )
}

fn environment() -> RequestEnvironment {
    RequestEnvironment::new(
        replay_compatibility(),
        RequestEnvironmentFingerprint::new([0xab; 32]),
    )
}

fn timing() -> DispatchedRequestTiming {
    DispatchedRequestTiming::new(
        UnixMillis::new(100),
        Some(ElapsedMillis::new(10)),
        Some(ElapsedMillis::new(20)),
        ElapsedMillis::new(30),
    )
    .unwrap_or_else(|error| panic!("timing fixture: {error}"))
}

/// TIM-2/TIM-3: both request owners retain one lossless tagged wire form without repeating a
/// turn or semantic-boundary identity inside the owner itself.
#[test]
fn tim_3_attempt_wire_round_trips_both_owner_variants() {
    let owners = [
        RequestAttemptOwner::AgentStep {
            step_id: step("turn-a", 1),
        },
        RequestAttemptOwner::Compaction {
            compaction_id: compaction("compact-a"),
        },
    ];

    for (index, owner) in owners.into_iter().enumerate() {
        let authorization = RequestAttemptAuthorized::new(
            attempt(&format!("attempt-{index}")),
            owner,
            entry("boundary-a"),
            environment(),
            UnixMillis::new(90),
        );
        let json = serde_json::to_string(&authorization)
            .unwrap_or_else(|error| panic!("encode authorization: {error}"));
        let decoded = serde_json::from_str::<RequestAttemptAuthorized>(&json)
            .unwrap_or_else(|error| panic!("decode authorization: {error}"));
        assert_eq!(decoded, authorization);
    }

    let terminals = [
        RequestAttemptTerminal::new(
            attempt("not-dispatched"),
            RequestAttemptTerminalState::NotDispatched {
                outcome: RequestNotDispatchedOutcome::EncodingFailed,
            },
        )
        .unwrap_or_else(|error| panic!("not-dispatched terminal: {error}")),
        RequestAttemptTerminal::new(
            attempt("dispatched"),
            RequestAttemptTerminalState::Dispatched {
                timing: timing(),
                outcome: RequestDispatchedOutcome::Completed {
                    stop_reason: StopReason::EndOfTurn,
                },
                usage: TokenUsage::Complete(TokenCounts {
                    input: 10,
                    cached_input: Some(8),
                    cache_write_input: Some(0),
                    output: 2,
                    reasoning_output: Some(1),
                    total: 12,
                }),
            },
        )
        .unwrap_or_else(|error| panic!("dispatched terminal: {error}")),
    ];
    for terminal in terminals {
        let json = serde_json::to_string(&terminal)
            .unwrap_or_else(|error| panic!("encode terminal: {error}"));
        let decoded = serde_json::from_str::<RequestAttemptTerminal>(&json)
            .unwrap_or_else(|error| panic!("decode terminal: {error}"));
        assert_eq!(decoded, terminal);
    }
}

/// TIM-2: deserialization cannot bypass monotonic milestone ordering.
#[test]
fn tim_2_invalid_milestone_order_is_refused_by_constructor_and_wire() {
    assert_eq!(
        DispatchedRequestTiming::new(
            UnixMillis::new(100),
            None,
            Some(ElapsedMillis::new(1)),
            ElapsedMillis::new(2),
        ),
        Err(RequestTimingError::FirstOutputWithoutHeaders)
    );
    assert_eq!(
        DispatchedRequestTiming::new(
            UnixMillis::new(100),
            Some(ElapsedMillis::new(5)),
            Some(ElapsedMillis::new(4)),
            ElapsedMillis::new(6),
        ),
        Err(RequestTimingError::MilestonesOutOfOrder)
    );

    let invalid = r#"{
        "dispatched_at":100,
        "headers_after_ms":10,
        "first_output_after_ms":20,
        "terminal_after_ms":15
    }"#;
    assert!(serde_json::from_str::<DispatchedRequestTiming>(invalid).is_err());
}

/// TIM-3: fixed-width canonical digest syntax and durable elapsed values revalidate at their
/// constructors instead of accepting malformed journal input.
#[test]
fn tim_3_id_fingerprint_and_elapsed_constructors_guard_the_wire() {
    assert_eq!(
        RequestAttemptId::new("  "),
        Err(RequestTimingError::EmptyRequestAttemptId)
    );
    assert_eq!(
        CompactionId::new(""),
        Err(RequestTimingError::EmptyCompactionId)
    );
    assert_eq!(
        ElapsedMillis::try_from(Duration::MAX),
        Err(RequestTimingError::ElapsedMillisOutOfRange)
    );

    let fingerprint = RequestEnvironmentFingerprint::new([0xab; 32]);
    let json = serde_json::to_string(&fingerprint)
        .unwrap_or_else(|error| panic!("encode fingerprint: {error}"));
    assert_eq!(json, format!("\"{}\"", "ab".repeat(32)));
    assert_eq!(
        serde_json::from_str::<RequestEnvironmentFingerprint>(&json)
            .unwrap_or_else(|error| panic!("decode fingerprint: {error}")),
        fingerprint
    );
    assert!(
        serde_json::from_str::<RequestEnvironmentFingerprint>(&format!("\"{}\"", "AB".repeat(32)))
            .is_err()
    );
}

fn dispatched_state(usage: TokenUsage) -> RequestAttemptTerminalState {
    RequestAttemptTerminalState::Dispatched {
        timing: timing(),
        outcome: RequestDispatchedOutcome::TransportFailed,
        usage,
    }
}

fn counts() -> TokenCounts {
    TokenCounts {
        input: 10,
        cached_input: Some(4),
        cache_write_input: Some(2),
        output: 3,
        reasoning_output: Some(1),
        total: 13,
    }
}

/// TIM-3: durable usage rechecks provider count relationships and complete coverage, while a
/// legitimately all-field partial report remains partial.
#[test]
fn tim_3_terminal_usage_is_internally_consistent() {
    let mut bad_cached = counts();
    bad_cached.cached_input = Some(11);
    let mut bad_write = counts();
    bad_write.cache_write_input = Some(11);
    let mut bad_reasoning = counts();
    bad_reasoning.reasoning_output = Some(4);
    let mut bad_total = counts();
    bad_total.total = 99;
    let mut incomplete = counts();
    incomplete.reasoning_output = None;

    for (usage, field) in [
        (TokenUsage::Complete(bad_cached), "cached_input"),
        (TokenUsage::Complete(bad_write), "cache_write_input"),
        (TokenUsage::Complete(bad_reasoning), "reasoning_output"),
        (TokenUsage::Complete(bad_total), "total"),
        (TokenUsage::Complete(incomplete), "coverage"),
    ] {
        assert_eq!(
            RequestAttemptTerminal::new(attempt("invalid"), dispatched_state(usage)),
            Err(RequestTimingError::InvalidUsage { field })
        );
    }

    RequestAttemptTerminal::new(
        attempt("partial-all-fields"),
        dispatched_state(TokenUsage::Partial(counts())),
    )
    .unwrap_or_else(|error| panic!("all-field partial remains valid: {error}"));

    let invalid_wire = r#"{
        "attempt_id":"invalid-wire",
        "terminal":{
            "state":"dispatched",
            "timing":{
                "dispatched_at":100,
                "headers_after_ms":1,
                "first_output_after_ms":null,
                "terminal_after_ms":2
            },
            "outcome":{"kind":"transport_failed"},
            "usage":{
                "coverage":"complete",
                "counts":{
                    "input":10,
                    "cached_input":null,
                    "cache_write_input":0,
                    "output":2,
                    "reasoning_output":0,
                    "total":12
                }
            }
        }
    }"#;
    assert!(serde_json::from_str::<RequestAttemptTerminal>(invalid_wire).is_err());
}
