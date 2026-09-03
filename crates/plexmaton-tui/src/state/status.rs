//! The status line: what the workspace says about itself in the composer's bottom border.
//!
//! One slot, one message at a time. At rest it names where the process runs; after a key that
//! asked a question, it carries the answer until the next key. Nothing here is session state,
//! which is why it is not an event: the runtime never has a reason to write to it.

use super::ViewState;
use crate::surface::SurfaceTree;

/// What the status line is saying, and why.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusNote {
    /// Nothing asked: the line shows the working directory.
    #[default]
    Quiet,
    /// `Ctrl-C` found nothing to clear, so the line says how to leave.
    QuitHint,
    /// `Ctrl-D` was pressed once; the next press leaves.
    QuitArmed,
}

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
    /// The second press in a row: the user leaves.
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

    /// One press of the quit chord: the first asks, the second confirms (INV-7).
    ///
    /// The state does not change on confirmation, because the process is leaving and a repaint
    /// of a screen about to be released is work nobody sees.
    pub fn press_quit(&mut self) -> QuitPress {
        if self.status.note() == StatusNote::QuitArmed {
            return QuitPress::Confirmed;
        }
        self.status.set_note(StatusNote::QuitArmed);
        self.touch();
        QuitPress::Asked
    }

    /// `Ctrl-C`: clears the draft under the cursor, names its conversation for interruption, and
    /// with nothing to clear says how to leave.
    ///
    /// The shell habit may discard a draft and stop work, but it never ends the session.
    pub fn interrupt(&mut self, surfaces: &SurfaceTree) -> Option<plexmaton_core::AgentId> {
        let target = match self.focus.resolve(surfaces) {
            Some(crate::surface::SurfaceId::Inspector) => {
                self.inspector().map(|inspector| inspector.agent)
            }
            _ => self.agents.primary().map(|agent| agent.id.clone()),
        };
        if let Some(to) = self.text_target(surfaces)
            && let Some(composer) = self.composers.get_mut(&to)
            && composer.clear()
        {
            // `Ctrl-C` is another key after an armed quit, so clearing a draft also withdraws the
            // question. Do both before one touch: they are one user-visible transition (INV-7,
            // FR-1).
            self.status.set_note(StatusNote::Quiet);
            self.touch();
            return target;
        }
        if self.status.set_note(StatusNote::QuitHint) {
            self.touch();
        }
        target
    }

    /// Any key that is not the quit chord withdraws what the status line asked.
    pub fn settle_status(&mut self) {
        if self.status.set_note(StatusNote::Quiet) {
            self.touch();
        }
    }
}
