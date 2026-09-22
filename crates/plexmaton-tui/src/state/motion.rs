//! The motion clock's phase, held where every renderer can read it (MOT-1).

use super::ViewState;

impl ViewState {
    pub(crate) const fn motion_phase(&self) -> u16 {
        self.motion_phase
    }

    pub(crate) const fn set_motion_phase(&mut self, phase: u16) {
        self.motion_phase = phase;
    }
}
