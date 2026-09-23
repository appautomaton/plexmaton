//! MOT-1: one visible-only motion clock for the whole workspace.
//!
//! Anything that moves reads this clock's phase and adds its own visibility to [`Workspace::moving`];
//! nothing that moves owns a timer of its own. A phase change invalidates presentation, never
//! semantic state (MOT-3).

use super::Workspace;
use std::time::{Duration, Instant};

/// Fifteen phases a second, the rate the effort rail was designed at.
const TICK: Duration = Duration::from_millis(67);
/// Phases in one cycle, 32 s; every motion sequence divides it.
const CYCLE: u128 = 480;

#[derive(Debug)]
pub(super) struct Motion {
    start: Instant,
    next: Instant,
}

impl Workspace {
    /// Whether any visible cell moves. Each moving thing answers for its own visibility.
    fn moving(&self) -> bool {
        self.effort_moving() || self.state.activity_moves()
    }

    /// Arm the one shared deadline while anything visible moves; nothing moving owns no wake.
    pub fn motion_deadline(&mut self, now: Instant) -> Option<Instant> {
        if !self.moving() {
            self.motion = None;
            return None;
        }
        Some(
            self.motion
                .get_or_insert(Motion {
                    start: now,
                    next: now + TICK,
                })
                .next,
        )
    }

    /// Late wakes coalesce into the current phase, without queuing missed frames.
    pub fn advance_motion(&mut self, now: Instant) {
        if !self.moving() {
            self.motion = None;
            return;
        }
        let Some(motion) = &mut self.motion else {
            return;
        };
        if now < motion.next {
            return;
        }
        let phase =
            (now.saturating_duration_since(motion.start).as_millis() * 15 / 1000 % CYCLE) as u16;
        motion.next = now + TICK;
        self.state.tick_activity(now);
        if phase != self.state.motion_phase() {
            self.state.set_motion_phase(phase);
            self.painted = None;
        }
    }
}
