//! Status presentation and the system question that owns the terminal's last row.
//!
//! The composition supplies decoded script rows or the cwd baseline. A bounded system hint
//! overrides only the final visible row. These are presentation values, never session facts.

use std::time::{Duration, Instant};

use super::ViewState;
use crate::surface::SurfaceTree;

/// What the status line is saying, and why.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum StatusNote {
    /// Nothing asked: the line shows the working directory.
    #[default]
    Quiet,
    /// Something is waiting for the same gesture a second time.
    Armed(Armed),
}

/// One action that will happen if the user repeats the gesture that asked for it.
///
/// This is the workspace's one place for "do that again and I will do it": the terminal's last row,
/// in `ActionRequired`, saying the cost before it is paid. The quit chord was the first of these
/// and for a while the only one, which is why the concept was spelled `QuitArmed` — but a second
/// arrived the moment a conversation switch had a working child to lose, and a second place to put
/// the same question would have been a second grammar for the user to learn and a second lifecycle
/// to keep from drifting. A third belongs here too.
///
/// What every member shares: the gesture that arms it is the gesture that confirms it, the line
/// states what repeating it costs, and doing something else instead disarms it. What they do not
/// share is how they stop waiting — a chord expires on a deadline, a switch waits until the user
/// asks for something else — so `deadline` is optional rather than a field every member carries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Armed {
    /// `Ctrl-D` was pressed once; another press before `deadline` leaves.
    Quit { deadline: Instant },
    /// A conversation switch would stop this working child; the same choice again performs it.
    Switch { child: String },
}

impl Armed {
    /// The one row this asks for, in the user's terms: what happens, and what it costs.
    #[must_use]
    pub fn message(&self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Quit { .. } => std::borrow::Cow::Borrowed("press Ctrl-D again to quit"),
            Self::Switch { child } => std::borrow::Cow::Owned(format!(
                "{child} is still working. Choose again to switch — it stops, and its work is saved."
            )),
        }
    }

    /// This question, wrapped to the row width it is read at.
    ///
    /// One indent column, matching the row's leading space, so a wrapped second line sits under
    /// the first rather than against the terminal edge.
    #[must_use]
    pub fn lines(&self, width: u16) -> Vec<String> {
        let mut lines =
            crate::state::wrap_line(&self.message(), usize::from(width.saturating_sub(1).max(1)));
        lines.truncate(ARMED_LINES);
        lines
    }

    /// When this stops waiting on its own, for the members that do.
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        match self {
            Self::Quit { deadline } => Some(*deadline),
            Self::Switch { .. } => None,
        }
    }
}

/// Observed clipboard transport result; terminal delivery has no acceptance acknowledgment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopyReceipt {
    /// The local native helper accepted the source.
    Copied,
    /// The terminal or multiplexer accepted a send, without confirming the user's clipboard.
    Sent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CopyNotice {
    receipt: CopyReceipt,
    deadline: Instant,
}

/// Rows one armed question may wrap onto. Past this it is asking too much to be one question.
const ARMED_LINES: usize = 2;

/// How long a first `Ctrl-D` remains eligible for confirmation (INV-7).
pub const QUIT_CHORD_WINDOW: Duration = Duration::from_secs(1);

/// The status line's state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Status {
    working_directory: Option<String>,
    note: StatusNote,
    footer: Footer,
    copy: Option<CopyNotice>,
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

    /// Rows this line needs at `width`, including an armed question too long to fit on one.
    ///
    /// The quit chord never needed this — six words fit anywhere. A switch names a child and says
    /// what stopping it costs, and at 60 columns that is two rows. Truncating would cut the half
    /// that says what to do, which is the half the row exists for, so the line grows instead.
    pub(crate) fn rows(&self, width: u16) -> u16 {
        let footer = match &self.footer {
            Footer::Script { text, max_rows } => (text.lines().len() as u16).min(*max_rows).max(1),
            Footer::Default | Footer::Failed(_) => 1,
        };
        footer.max(self.armed_rows(width))
    }

    /// The rows an armed question wraps onto, bounded so one sentence cannot take the workspace.
    pub(crate) fn armed_rows(&self, width: u16) -> u16 {
        self.armed().map_or(1, |armed| {
            u16::try_from(armed.lines(width).len()).unwrap_or(1).max(1)
        })
    }
    /// Where the process runs, as the composition root chose to show it.
    #[must_use]
    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }

    /// What the line is saying right now.
    #[must_use]
    pub fn note(&self) -> &StatusNote {
        &self.note
    }

    /// The action waiting for a second gesture, if one is.
    #[must_use]
    pub const fn armed(&self) -> Option<&Armed> {
        match &self.note {
            StatusNote::Quiet => None,
            StatusNote::Armed(armed) => Some(armed),
        }
    }

    /// When whatever the line is saying expires, if it expires at all.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.armed()
            .and_then(Armed::deadline)
            .into_iter()
            .chain(self.copy.map(|notice| notice.deadline))
            .min()
    }

    pub(crate) fn copy_receipt(&self) -> Option<CopyReceipt> {
        self.copy.map(|notice| notice.receipt)
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
    /// Puts one action on the last row to wait for the gesture that asked for it, again.
    ///
    /// One entry point for every member of [`Armed`], so a new kind of confirmation cannot arrive
    /// with its own place to live. Arming replaces whatever was waiting: two questions on one row
    /// would be one the user answers by accident.
    pub fn arm(&mut self, armed: Armed) {
        if self.status.set_note(StatusNote::Armed(armed)) {
            self.touch();
        }
    }

    /// Withdraws a waiting switch because the user asked for something else.
    ///
    /// Only a switch: the quit chord withdraws itself on its deadline, and a caller that cleared
    /// the row wholesale would cancel a chord the user is halfway through.
    pub fn disarm_switch(&mut self) {
        if matches!(self.status.armed(), Some(Armed::Switch { .. }))
            && self.status.set_note(StatusNote::Quiet)
        {
            self.touch();
        }
    }

    /// The state does not change on confirmation, because the process is leaving and a repaint
    /// of a screen about to be released is work nobody sees.
    pub fn press_quit(&mut self, now: Instant) -> QuitPress {
        if matches!(
            self.status.armed(),
            Some(Armed::Quit { deadline }) if now < *deadline
        ) {
            return QuitPress::Confirmed;
        }
        self.status.set_note(StatusNote::Armed(Armed::Quit {
            deadline: now + QUIT_CHORD_WINDOW,
        }));
        self.touch();
        QuitPress::Asked
    }

    /// `Ctrl-C`: clears the addressed conversation's draft or names it for interruption.
    ///
    /// Clearing consumes the key, so one press never both discards text and stops work. Either
    /// path withdraws a pending quit question, and neither path ends the session (INV-7).
    pub fn interrupt(&mut self, surfaces: &SurfaceTree) -> Option<plexmaton_core::AgentId> {
        // TRE-1/INV-7: a modal blocks hidden conversation actions, not withdrawal of the
        // process-wide quit question. An admitted history write also remains owned.
        if self.conversation_tree_open()
            || surfaces
                .get(crate::surface::SurfaceId::Agents)
                .is_some_and(|surface| surface.kind.blocks_below())
        {
            if self.status.set_note(StatusNote::Quiet) {
                self.touch();
            }
            return None;
        }
        let target = match self.focus.resolve(surfaces) {
            Some(crate::surface::SurfaceId::Inspector) => {
                self.inspector().map(|inspector| inspector.agent)
            }
            _ => self.agents.primary().map(|agent| agent.id.clone()),
        };
        let editable = target.as_ref().is_some_and(|to| {
            self.agents
                .primary()
                .is_some_and(|primary| primary.id == *to)
                || self.text_target(surfaces).as_ref() == Some(to)
        });
        if let Some(to) = target.as_ref()
            && editable
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

    pub(crate) fn clear_copy_receipt(&mut self) {
        if self.status.copy.take().is_some() {
            self.touch();
        }
    }

    pub(crate) fn report_copy(&mut self, receipt: CopyReceipt, now: Instant) {
        self.status.copy = Some(CopyNotice {
            receipt,
            deadline: now + Duration::from_secs(2),
        });
        self.touch();
    }

    /// Expires owned status hints once at their monotonic deadlines.
    pub fn expire_note(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if self
            .status
            .armed()
            .and_then(Armed::deadline)
            .is_some_and(|deadline| now >= deadline)
        {
            changed |= self.status.set_note(StatusNote::Quiet);
        }
        if self
            .status
            .copy
            .is_some_and(|notice| now >= notice.deadline)
        {
            self.status.copy = None;
            changed = true;
        }
        if changed {
            self.touch();
        }
        changed
    }
}
