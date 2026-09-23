//! How long the primary agent's turn has run and when its route was last heard, for the activity
//! line's elapsed and quiet readings (ui-ux §input, COM-5).
//!
//! Presentation only: nothing here moves the semantic revision, and a replayed journal reproduces
//! the same rows because the rows never read these instants — only the frame drawn now does.

use std::time::{Duration, Instant};

use plexmaton_core::AgentStatus;

use super::{CurrentWork, ViewState};

/// Silence shorter than this is ordinary streaming cadence and says nothing.
pub(crate) const QUIET_AFTER: Duration = Duration::from_secs(5);

/// The open turn's count, or nothing between turns.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ActivityClock {
    turn: Option<TurnClock>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TurnClock {
    /// Time counted before the latest resume.
    counted: Duration,
    /// When counting last resumed; `None` while the turn waits on the user.
    resumed: Option<Instant>,
    /// When the primary's route was last heard, or counting last resumed.
    heard: Instant,
    /// The instant the next frame is drawn for.
    now: Instant,
}

impl ViewState {
    /// Note a batch of producer events applied at `now`. The count opens with the turn and survives
    /// the gaps between its steps, stops while the turn waits on the user, and is forgotten when the
    /// turn ends. `heard` says whether the batch carried anything from the primary's own route.
    pub(crate) fn observe_activity(&mut self, now: Instant, heard: bool) {
        let waiting = match self.turn_phase() {
            TurnPhase::Closed => {
                self.activity = ActivityClock::default();
                return;
            }
            TurnPhase::Working => false,
            TurnPhase::WaitingOnUser => true,
        };
        let turn = self.activity.turn.get_or_insert(TurnClock {
            counted: Duration::ZERO,
            resumed: Some(now),
            heard: now,
            now,
        });
        match (waiting, turn.resumed) {
            (true, Some(resumed)) => {
                turn.counted += now.saturating_duration_since(resumed);
                turn.resumed = None;
            }
            // The user's wait is not the route's silence either.
            (false, None) => {
                turn.resumed = Some(now);
                turn.heard = now;
            }
            _ => {}
        }
        if heard {
            turn.heard = now;
        }
        turn.now = now;
    }

    /// The motion clock's wake: the instant the next frame is drawn for.
    pub(crate) fn tick_activity(&mut self, now: Instant) {
        if let Some(turn) = &mut self.activity.turn {
            turn.now = now;
        }
    }

    /// How long the turn has run, not counting its waits on the user.
    pub(crate) fn activity_elapsed(&self) -> Option<Duration> {
        let turn = self.activity.turn.as_ref()?;
        let running = turn.resumed.map_or(Duration::ZERO, |resumed| {
            turn.now.saturating_duration_since(resumed)
        });
        Some(turn.counted + running)
    }

    /// How long the primary's route has said nothing, once that is long enough to be worth saying.
    /// A turn waiting on the user is not quiet: it is waiting.
    pub(crate) fn activity_quiet(&self) -> Option<Duration> {
        let turn = self.activity.turn.as_ref()?;
        turn.resumed?;
        let quiet = turn.now.saturating_duration_since(turn.heard);
        (quiet >= QUIET_AFTER).then_some(quiet)
    }

    /// Whether the activity line has a mark that moves: some work, and not a wait on the user.
    pub(crate) fn activity_moves(&self) -> bool {
        self.current_work()
            .is_some_and(|work| work != CurrentWork::ApprovalRequired)
    }

    /// A turn is open while the primary runs or waits on its own work, and while it compacts; the
    /// label can go blank between steps, while queued tools wait to start, without ending it. An
    /// open turn is held while an approval or a question only the user can answer is outstanding.
    fn turn_phase(&self) -> TurnPhase {
        let Some(primary) = self.agents.primary() else {
            return TurnPhase::Closed;
        };
        let work = self.current_work();
        let open =
            matches!(primary.status, AgentStatus::Running | AgentStatus::Waiting) || work.is_some();
        if !open {
            TurnPhase::Closed
        } else if work == Some(CurrentWork::ApprovalRequired)
            || self
                .attention
                .iter()
                .any(|request| request.agent_id == primary.id)
        {
            TurnPhase::WaitingOnUser
        } else {
            TurnPhase::Working
        }
    }
}

enum TurnPhase {
    Closed,
    Working,
    WaitingOnUser,
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
    use crate::test_support::Conversation;
    use plexmaton_core::{
        AgentId, AgentStatus, ApprovalId, ApprovalReason, AttentionId, AttentionRequest,
        ConversationEvent, ToolCallId, ToolCapability,
    };

    fn primary() -> AgentId {
        AgentId::new("agent-a").expect("the canonical primary")
    }

    fn at(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    /// COM-5: the count starts with the turn and quiet appears only after five silent seconds;
    /// the turn's end forgets both.
    #[test]
    fn the_turn_is_counted_from_its_start_and_forgotten_when_it_ends() {
        let mut conversation = Conversation::canonical();
        let start = Instant::now();
        conversation.state.observe_activity(start, true);
        conversation.state.observe_activity(at(start, 3), false);
        conversation.state.tick_activity(at(start, 4));
        assert_eq!(
            conversation.state.activity_elapsed(),
            Some(Duration::from_secs(4))
        );
        assert_eq!(
            conversation.state.activity_quiet(),
            None,
            "four silent seconds say nothing"
        );
        conversation.state.tick_activity(at(start, 9));
        assert_eq!(
            conversation.state.activity_quiet(),
            Some(Duration::from_secs(9))
        );

        conversation.emit(ConversationEvent::AgentStatusChanged {
            agent_id: primary(),
            status: AgentStatus::Idle,
        });
        conversation.state.observe_activity(at(start, 10), true);
        assert_eq!(
            conversation.state.activity_elapsed(),
            None,
            "the ended turn forgets its count"
        );
        assert!(!conversation.state.activity_moves());
    }

    /// COM-5: the gap between steps, while queued tools wait to start and nothing is named, is
    /// still the turn; the count carries through it rather than starting over.
    #[test]
    fn the_count_carries_through_the_gap_between_steps() {
        let mut conversation = Conversation::canonical();
        let start = Instant::now();
        conversation.state.observe_activity(start, true);
        conversation.emit(ConversationEvent::AgentStatusChanged {
            agent_id: primary(),
            status: AgentStatus::Waiting,
        });
        assert_eq!(
            conversation.state.current_work(),
            None,
            "the gap names no work"
        );
        conversation.state.observe_activity(at(start, 3), true);
        assert_eq!(
            conversation.state.activity_elapsed(),
            Some(Duration::from_secs(3))
        );
        conversation.emit(ConversationEvent::AgentStatusChanged {
            agent_id: primary(),
            status: AgentStatus::Running,
        });
        conversation.state.observe_activity(at(start, 6), true);
        conversation.state.tick_activity(at(start, 8));
        assert_eq!(
            conversation.state.activity_elapsed(),
            Some(Duration::from_secs(8)),
            "the next step continues the turn's count"
        );
    }

    /// COM-5: an approval or a question holds the turn on the user. The count stops for it and
    /// resumes where it stopped once answered, and the wait is not the route being quiet.
    #[test]
    fn the_count_stops_while_the_turn_waits_on_the_user() {
        let approval = AttentionRequest::Approval {
            reason: ApprovalReason::PermissionRequired,
            remember: None,
            approval_id: ApprovalId::new("approval-a-wait").expect("fixture"),
            call_id: ToolCallId::new("tool-a-wait").expect("fixture"),
            tool: "edit".into(),
            capabilities: vec![ToolCapability::FileWrite],
            detail: "Approve writing the findings file.".into(),
        };
        let question = AttentionRequest::Clarification {
            summary: "Which branch should the findings go on?".into(),
        };
        for (name, request) in [("approval", approval), ("question", question)] {
            let mut conversation = Conversation::canonical();
            let start = Instant::now();
            conversation.state.observe_activity(start, true);
            let attention_id = AttentionId::new(format!("attention-a-{name}")).expect("fixture");
            conversation.emit(ConversationEvent::AttentionRequested {
                agent_id: primary(),
                attention_id: attention_id.clone(),
                request,
            });
            conversation.state.observe_activity(at(start, 4), true);
            conversation.state.tick_activity(at(start, 64));
            assert_eq!(
                conversation.state.activity_elapsed(),
                Some(Duration::from_secs(4)),
                "{name}: a minute waiting on the user is not counted"
            );
            assert_eq!(
                conversation.state.activity_quiet(),
                None,
                "{name}: and waiting is not quiet"
            );

            conversation.emit(ConversationEvent::AttentionResolved {
                agent_id: primary(),
                attention_id,
            });
            conversation.state.observe_activity(at(start, 64), true);
            conversation.state.tick_activity(at(start, 70));
            assert_eq!(
                conversation.state.activity_elapsed(),
                Some(Duration::from_secs(10)),
                "{name}: the count resumes where it stopped"
            );
            assert_eq!(
                conversation.state.activity_quiet(),
                Some(Duration::from_secs(6)),
                "{name}: quiet counts from the answer, not from before the wait"
            );
        }
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
