//! When the primary agent's current work began and when its producer was last heard, for the
//! activity line's elapsed and quiet readings (ui-ux §input).
//!
//! Presentation only: nothing here moves the semantic revision, and a replayed journal reproduces
//! the same rows because the rows never read these instants — only the frame drawn now does.

use std::time::{Duration, Instant};

use super::{CurrentWork, ViewState};

/// Silence shorter than this is ordinary streaming cadence and says nothing.
pub(crate) const QUIET_AFTER: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ActivityClock {
    since: Option<Instant>,
    heard: Option<Instant>,
    now: Option<Instant>,
}

impl ViewState {
    /// Note a batch of producer events applied at `now`: work that has just begun starts its
    /// count, work that has ended forgets it, and any event is the producer being heard.
    pub(crate) fn observe_activity(&mut self, now: Instant, heard: bool) {
        let working = self.current_work().is_some();
        let clock = &mut self.activity;
        if !working {
            *clock = ActivityClock::default();
            return;
        }
        clock.since.get_or_insert(now);
        if heard || clock.heard.is_none() {
            clock.heard = Some(now);
        }
        clock.now = Some(now);
    }

    /// The motion clock's wake: the instant the next frame is drawn for.
    pub(crate) fn tick_activity(&mut self, now: Instant) {
        if self.activity.since.is_some() {
            self.activity.now = Some(now);
        }
    }

    /// How long the current work has run, once its beginning was seen.
    pub(crate) fn activity_elapsed(&self) -> Option<Duration> {
        let clock = &self.activity;
        Some(clock.now?.saturating_duration_since(clock.since?))
    }

    /// How long the producer has said nothing, once that is long enough to be worth saying.
    pub(crate) fn activity_quiet(&self) -> Option<Duration> {
        let clock = &self.activity;
        let quiet = clock.now?.saturating_duration_since(clock.heard?);
        (quiet >= QUIET_AFTER).then_some(quiet)
    }

    /// Whether the activity line has a mark that moves: some work, and not a wait on the user.
    pub(crate) fn activity_moves(&self) -> bool {
        self.current_work()
            .is_some_and(|work| work != CurrentWork::ApprovalRequired)
    }
}

/// `11s`, `2m 11s`, `1h 2m`: whole units, the largest two.
pub(crate) fn elapsed_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::elapsed_label;
    use plexmaton_core::{
        AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    };

    /// ui-ux §input: the count starts when work is first seen, holds while it runs, and is
    /// forgotten when it ends; quiet appears only after five silent seconds.
    #[test]
    fn work_is_counted_from_its_start_and_forgotten_when_it_ends() {
        let mut state = crate::test_support::canonical_state();
        let start = Instant::now();
        state.observe_activity(start, true);
        state.observe_activity(start + Duration::from_secs(3), false);
        state.tick_activity(start + Duration::from_secs(4));
        assert_eq!(state.activity_elapsed(), Some(Duration::from_secs(4)));
        assert_eq!(
            state.activity_quiet(),
            None,
            "four silent seconds say nothing"
        );
        state.tick_activity(start + Duration::from_secs(9));
        assert_eq!(state.activity_quiet(), Some(Duration::from_secs(9)));

        let _ = state.apply(ConversationEventEnvelope {
            sequence: EventSequence::new(u64::MAX / 2),
            event: ConversationEvent::AgentStatusChanged {
                agent_id: AgentId::new("agent-a").expect("primary"),
                status: AgentStatus::Idle,
            },
        });
        state.observe_activity(start + Duration::from_secs(10), true);
        assert_eq!(
            state.activity_elapsed(),
            None,
            "ended work forgets its count"
        );
        assert!(!state.activity_moves());
    }

    /// ui-ux §input: elapsed reads in whole units, the largest two.
    #[test]
    fn elapsed_reads_in_the_largest_two_whole_units() {
        for (seconds, label) in [
            (0, "0s"),
            (11, "11s"),
            (59, "59s"),
            (60, "1m 0s"),
            (131, "2m 11s"),
            (3599, "59m 59s"),
            (3600, "1h 0m"),
            (3720, "1h 2m"),
        ] {
            assert_eq!(elapsed_label(Duration::from_secs(seconds)), label);
        }
    }
}
