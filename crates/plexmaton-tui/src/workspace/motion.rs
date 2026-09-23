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
        self.effort_moving()
            || self.state.activity_moves()
            || self.state.greeting_moment().is_some()
    }

    /// Greet the user with the mark, once: launch calls this when it opens onto an empty
    /// conversation, and nothing else does, so `/new` and a reopened conversation are not greeted.
    pub fn greet(&mut self, now: Instant) {
        self.greeting = Some(now);
        self.state.set_greeting(Some(0));
    }

    /// The terminal's cell in pixels, so the mark comes out square; `None` when it reports none.
    pub fn set_cell_size(&mut self, cell: Option<crate::render::mark::CellSize>) {
        self.state.set_cell_size(cell);
    }

    /// Moves the greeting to `now`, ending it once it has played or the conversation has begun.
    /// Reports whether what it draws changed.
    pub(super) fn advance_greeting(&mut self, now: Instant) -> bool {
        let Some(start) = self.greeting else {
            return false;
        };
        let phase = u16::try_from(now.saturating_duration_since(start).as_millis() * 15 / 1000)
            .unwrap_or(u16::MAX);
        if !self.state.primary_is_empty() || crate::render::mark::greeting(phase).is_none() {
            self.greeting = None;
            self.state.set_greeting(None);
            return true;
        }
        if self.state.greeting_phase() == Some(phase) {
            return false;
        }
        self.state.set_greeting(Some(phase));
        true
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
        if self.advance_greeting(now) {
            self.painted = None;
        }
        if phase != self.state.motion_phase() {
            self.state.set_motion_phase(phase);
            self.painted = None;
        }
    }
}
