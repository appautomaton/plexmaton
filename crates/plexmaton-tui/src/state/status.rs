//! Status presentation and the system question that owns the terminal's last row.
//!
//! The composition supplies decoded script rows or the cwd baseline. A bounded system hint
//! overrides only the final visible row. These are presentation values, never session facts.

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
}

/// How long a first `Ctrl-D` remains eligible for confirmation (INV-7).
pub const QUIT_CHORD_WINDOW: Duration = Duration::from_secs(1);

/// The status line's state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Status {
    working_directory: Option<String>,
    note: StatusNote,
    footer: Footer,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum Footer {
    #[default]
    Default,
    Script {
        text: crate::StatusLineText,
        max_rows: u16,
    },
    Failed(String),
}

impl Status {
    pub(crate) fn footer(&self) -> &Footer {
        &self.footer
    }

    pub(crate) fn rows(&self) -> u16 {
        match &self.footer {
            Footer::Script { text, max_rows } => (text.lines().len() as u16).min(*max_rows).max(1),
            Footer::Default | Footer::Failed(_) => 1,
        }
    }
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
            StatusNote::QuitArmed { deadline } => Some(deadline),
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
    pub(crate) fn set_status_line(&mut self, text: crate::StatusLineText, max_rows: u16) {
        self.replace_footer(Footer::Script {
            text,
            max_rows: max_rows.clamp(1, 64),
        });
    }

    pub(crate) fn set_status_line_error(&mut self, error: String) {
        // Only the composition's bounded typed diagnostic should reach this entry point.
        let error: String = error
            .chars()
            .filter(|ch| !ch.is_control())
            .take(160)
            .collect();
        self.replace_footer(Footer::Failed(error));
    }

    fn replace_footer(&mut self, footer: Footer) {
        if self.status.footer != footer {
            self.status.footer = footer;
            self.touch();
        }
    }

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
            self.clear_skill_binding(to);
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

    /// Expires whatever the line is saying at its monotonic deadline, reporting a screen change.
    pub fn expire_note(&mut self, now: Instant) -> bool {
        if matches!(
            self.status.note(),
            StatusNote::QuitArmed { deadline } if now >= deadline
        ) && self.status.set_note(StatusNote::Quiet)
        {
            self.touch();
            return true;
        }
        false
    }
}
