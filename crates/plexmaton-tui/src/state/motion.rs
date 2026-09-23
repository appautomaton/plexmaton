//! The motion clock's phase, held where every renderer can read it (MOT-1), and the launch
//! greeting that plays on it.

use super::ViewState;
use crate::render::mark::{self, CellSize, Moment};

impl ViewState {
    pub(crate) const fn motion_phase(&self) -> u16 {
        self.motion_phase
    }

    pub(crate) const fn set_motion_phase(&mut self, phase: u16) {
        self.motion_phase = phase;
    }

    /// The greeting's moment, while it plays over a primary conversation that is still empty.
    pub(crate) fn greeting_moment(&self) -> Option<Moment> {
        if !self.primary_is_empty() {
            return None;
        }
        self.greeting.and_then(mark::greeting)
    }

    pub(crate) const fn greeting_phase(&self) -> Option<u16> {
        self.greeting
    }

    pub(crate) const fn set_greeting(&mut self, phase: Option<u16>) {
        self.greeting = phase;
    }

    /// Whether the primary conversation holds nothing yet: the only place the greeting plays.
    pub(crate) fn primary_is_empty(&self) -> bool {
        self.primary_agent()
            .is_some_and(|agent| agent.entries().next().is_none())
    }

    pub(crate) const fn cell_size(&self) -> Option<CellSize> {
        self.cell
    }

    pub(crate) const fn set_cell_size(&mut self, cell: Option<CellSize>) {
        self.cell = cell;
    }
}
