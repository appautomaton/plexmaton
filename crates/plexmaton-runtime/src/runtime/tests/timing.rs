use std::sync::Arc;

use plexmaton_agent::{
    Input, JournalEntryPayload, JournalRecord, ModelEvent, StopReason, TurnFinishedAt, UnixMillis,
};

use super::{FakeDriver, Script, agent_id, finish_active, runtime_with_clock};
use crate::runtime::clock::FixedWallClock;

/// TIM-1: the runtime supplies wall observations; neither the agent nor replay reads a clock.
#[tokio::test]
async fn runtime_clock_values_reach_durable_turn_boundaries() {
    let driver = FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
        StopReason::EndOfTurn,
    )])]);
    let observed_at = UnixMillis::new(1_788_000_000_123);
    let mut runtime = runtime_with_clock(driver, Arc::new(FixedWallClock(observed_at)));
    let _announced = runtime.try_next_event();

    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "timed".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submission: {error}"));
    let _events = finish_active(&mut runtime).await;

    assert!(
        runtime
            .agent
            .journal()
            .records()
            .iter()
            .any(|record| matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(
                        entry.payload,
                        JournalEntryPayload::TurnStarted {
                            accepted_at,
                            opened_at,
                            ..
                        } if accepted_at == observed_at && opened_at == observed_at
                    )
            ))
    );
    assert!(
        runtime
            .agent
            .journal()
            .records()
            .iter()
            .any(|record| matches!(
                record,
                JournalRecord::TurnFinished { fact, .. }
                    if fact.at == TurnFinishedAt::Observed {
                        completed_at: observed_at
                    }
            ))
    );
}
