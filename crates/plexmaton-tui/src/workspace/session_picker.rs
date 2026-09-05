//! Session discovery and palette activation share the existing modal's geometry and input routing.
use super::*;
use crate::{Point, PointerIntent, SessionChoice, SessionPickerStatus, SurfaceId};
use plexmaton_core::SessionId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PaletteChoice {
    Command(Command),
    Session(SessionId),
}

impl PaletteChoice {
    fn outcome(self) -> Outcome {
        match self {
            Self::Command(command) => Outcome {
                command: Some(command),
                ..Outcome::default()
            },
            Self::Session(session) => Outcome {
                resume: Some(session),
                ..Outcome::default()
            },
        }
    }
}

impl Workspace {
    /// Includes drafts temporarily displaced by edit/retry, so switching cannot silently lose them.
    pub fn has_unsent_input(&self) -> bool {
        self.state.has_unsent_input()
    }
    /// Opens the search surface before its owned loader supplies results.
    pub fn open_session_picker(&mut self) {
        self.state.open_session_picker();
    }

    /// Supplies bounded display choices; a result arriving after dismissal is ignored.
    pub fn set_session_choices(&mut self, entries: Vec<SessionChoice>, limited: bool) {
        self.state.set_session_choices(entries, limited);
    }

    /// Updates only the affected picker, never the conversation or multi-agent Notices.
    pub fn set_session_picker_status(&mut self, status: SessionPickerStatus) {
        self.state.session_picker_status(status);
    }

    /// Whether the user's permission to show picker work still exists.
    pub fn session_picker_open(&self) -> bool {
        self.state
            .command_palette()
            .is_some_and(|p| p.is_session_picker())
    }

    /// Returns focus without changing the selected durable session.
    pub fn close_session_picker(&mut self) {
        self.state.close_command_palette();
    }

    pub(super) fn activate_palette(&self) -> Outcome {
        self.state
            .command_palette()
            .and_then(|p| {
                if p.is_session_picker()
                    && !p.session_rows_visible(
                        self.surfaces.get(SurfaceId::CommandPalette)?.bounds.height,
                    )
                {
                    return None;
                }
                p.chosen_session()
                    .map(PaletteChoice::Session)
                    .or_else(|| p.chosen().map(PaletteChoice::Command))
            })
            .map_or_else(Outcome::default, PaletteChoice::outcome)
    }

    fn palette_hit(&self, at: Point) -> Option<PaletteChoice> {
        let bounds = self.surfaces.get(SurfaceId::CommandPalette)?.bounds;
        if at.x <= bounds.x || at.x >= bounds.right().saturating_sub(1) {
            return None;
        }
        let row = usize::from(at.y.checked_sub(bounds.y + 2)?);
        if at.y >= bounds.bottom().saturating_sub(1) {
            return None;
        }
        let palette = self.state.command_palette()?;
        let window = palette.choice_window(bounds.height);
        let index = {
            if row >= window.len()
                || (palette.is_session_picker() && !palette.session_rows_visible(bounds.height))
            {
                return None;
            }
            window.start + row
        };
        if index >= palette.match_count() {
            return None;
        }
        if let Some(sessions) = &palette.sessions {
            if !matches!(
                sessions.status,
                SessionPickerStatus::Ready | SessionPickerStatus::OpenFailed
            ) {
                return None;
            }
            sessions
                .matches(palette.filter().text())
                .get(index)
                .map(|entry| PaletteChoice::Session(entry.id.clone()))
        } else {
            palette
                .matches()
                .get(index)
                .copied()
                .map(PaletteChoice::Command)
        }
    }

    pub(super) fn palette_pointer(&mut self, pointer: PointerIntent) -> Option<Outcome> {
        match pointer {
            PointerIntent::Press {
                surface: SurfaceId::CommandPalette,
                at,
            } => {
                self.pressed_palette = self.palette_hit(at).map(|choice| (choice, at));
                self.pressed_palette.as_ref().map(|_| Outcome::default())
            }
            PointerIntent::Release {
                surface: SurfaceId::CommandPalette,
                at,
            } => {
                let (choice, original) = self.pressed_palette.take()?;
                Some(
                    if at == original && self.palette_hit(at) == Some(choice.clone()) {
                        choice.outcome()
                    } else {
                        Outcome::default()
                    },
                )
            }
            PointerIntent::Drag { .. }
            | PointerIntent::Cancel { .. }
            | PointerIntent::Suspend { .. } => {
                self.pressed_palette.take().map(|_| Outcome::default())
            }
            _ => None,
        }
    }
}
