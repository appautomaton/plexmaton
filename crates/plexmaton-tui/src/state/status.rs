//! The status line: what the workspace says about itself in the last row.
//!
//! One slot, one message at a time. At rest it names where the process runs; while the quit chord
//! is armed, it carries that bounded question. Nothing here is session state, which is why it is
//! not an event: the runtime never has a reason to write to it.

use std::time::{Duration, Instant};

use super::ViewState;
use crate::surface::SurfaceTree;

/// What the status line is saying, and why.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusNote {
    /// Nothing asked: the line shows the working directory.
    #[default]
    Quiet,
    /// `Ctrl-D` was pressed once; another press before `deadline` leaves.
    QuitArmed { deadline: Instant },
    /// A `/` opened an empty draft; the command list is one chord away until `deadline`.
    ///
    /// Offered, not demanded, which is why it carries `NewInformation` where the quit chord carries
    /// `ActionRequired`: the attention hierarchy keeps those two distinguishable in every palette.
    CommandHint { deadline: Instant },
}

/// How long a first `Ctrl-D` remains eligible for confirmation (INV-7).
pub const QUIT_CHORD_WINDOW: Duration = Duration::from_secs(1);

/// How long the command-list hint stays up.
///
/// Longer than the quit chord's second, because the two windows measure different things: that one
/// bounds a chord the user is already completing, while this one has to be read by someone who was
/// not looking at the last row. Any further typing takes it down anyway, so the extra seconds cost
/// one wake-up, not attention.
pub const COMMAND_HINT_WINDOW: Duration = Duration::from_secs(3);

/// The status line's state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Status {
    working_directory: Option<String>,
    note: StatusNote,
}

impl Status {
    /// Where the process runs, as the composition root chose to show it.
    #[must_use]
    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }

    /// What the line is saying right now.
    #[must_use]
    pub const fn note(&self) -> StatusNote {
        self.note
    }

    /// When whatever the line is saying expires, if it expires at all.
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        match self.note {
            StatusNote::Quiet => None,
            StatusNote::QuitArmed { deadline } | StatusNote::CommandHint { deadline } => {
                Some(deadline)
            }
        }
    }

    pub(super) fn set_working_directory(&mut self, path: String) {
        self.working_directory = Some(path);
    }

    /// Moves to `note`, reporting whether that changed anything.
    pub(super) fn set_note(&mut self, note: StatusNote) -> bool {
        if self.note == note {
            return false;
        }
        self.note = note;
        true
    }
}

/// What one press of the quit chord did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuitPress {
    /// The first press: the status line now asks for a second.
    Asked,
    /// A timely second press: the user leaves.
    Confirmed,
}

impl ViewState {
    /// The status line's state.
    #[must_use]
    pub const fn status(&self) -> &Status {
        &self.status
    }

    /// Names where the process runs; the status line shows it while nothing else is asked.
    pub fn set_working_directory(&mut self, path: String) {
        self.status.set_working_directory(path);
        self.touch();
    }

    /// One press of the quit chord: the first asks, a second within one second confirms (INV-7).
    ///
    /// The state does not change on confirmation, because the process is leaving and a repaint
    /// of a screen about to be released is work nobody sees.
    pub fn press_quit(&mut self, now: Instant) -> QuitPress {
        if matches!(
            self.status.note(),
            StatusNote::QuitArmed { deadline } if now < deadline
        ) {
            return QuitPress::Confirmed;
        }
        self.status.set_note(StatusNote::QuitArmed {
            deadline: now + QUIT_CHORD_WINDOW,
        });
        self.touch();
        QuitPress::Asked
    }

    /// `Ctrl-C`: clears the addressed conversation's draft or names it for interruption.
    ///
    /// Clearing consumes the key, so one press never both discards text and stops work. Either
    /// path withdraws a pending quit question, and neither path ends the session (INV-7).
    pub fn interrupt(&mut self, surfaces: &SurfaceTree) -> Option<plexmaton_core::AgentId> {
        let target = match self.focus.resolve(surfaces) {
            Some(crate::surface::SurfaceId::Inspector) => {
                self.inspector().map(|inspector| inspector.agent)
            }
            _ => self.agents.primary().map(|agent| agent.id.clone()),
        };
        if let Some(to) = target.as_ref()
            && let Some(composer) = self.inputs.get_mut(to)
            && composer.clear()
        {
            // Do both before one touch: they are one user-visible transition (INV-7, FR-1).
            self.status.set_note(StatusNote::Quiet);
            self.touch();
            return None;
        }
        if self.status.set_note(StatusNote::Quiet) {
            self.touch();
        }
        target
    }

    /// Offers the command list after a `/` opened an empty draft.
    ///
    /// Refused while the line already carries a question: one slot, one message, and an armed quit
    /// chord is a deadline the user is inside — replacing its text would hide a window still running.
    pub fn hint_command_palette(&mut self, now: Instant) -> bool {
        if !matches!(self.status.note(), StatusNote::Quiet) {
            return false;
        }
        let armed = self.status.set_note(StatusNote::CommandHint {
            deadline: now.checked_add(COMMAND_HINT_WINDOW).unwrap_or(now),
        });
        if armed {
            self.touch();
        }
        armed
    }

    /// Takes down a hint the next keystroke has answered, leaving a quit question alone.
    pub fn settle_command_hint(&mut self) -> bool {
        if !matches!(self.status.note(), StatusNote::CommandHint { .. }) {
            return false;
        }
        let cleared = self.status.set_note(StatusNote::Quiet);
        if cleared {
            self.touch();
        }
        cleared
    }

    /// Expires whatever the line is saying at its monotonic deadline, reporting a screen change.
    pub fn expire_note(&mut self, now: Instant) -> bool {
        if matches!(
            self.status.note(),
            StatusNote::QuitArmed { deadline } | StatusNote::CommandHint { deadline }
                if now >= deadline
        ) && self.status.set_note(StatusNote::Quiet)
        {
            self.touch();
            return true;
        }
        false
    }
}
