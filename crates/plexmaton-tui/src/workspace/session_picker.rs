//! Conversation discovery and palette activation share the existing modal's geometry and input routing.
use super::*;
use crate::{ConversationChoice, ConversationPickerStatus, Point, PointerIntent, SurfaceId};
use plexmaton_core::ConversationId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PaletteChoice {
    Command(Command),
    Conversation(ConversationId),
    Permission(crate::state::permissions::PermissionChoice),
}

impl Workspace {
    pub(super) fn step_palette(&mut self, direction: Direction) {
        let reading = self
            .state
            .command_palette()
            .and_then(crate::state::CommandPalette::permissions)
            .is_some_and(crate::state::permissions::PermissionPanel::is_reading);
        if reading {
            self.state.scroll(
                &self.surfaces,
                &self.metrics,
                SurfaceId::CommandPalette,
                if direction == Direction::Forward {
                    crate::ScrollDirection::Down
                } else {
                    crate::ScrollDirection::Up
                },
            );
        } else {
            self.state.step_command(direction == Direction::Forward);
        }
    }

    fn activate_palette_choice(&mut self, choice: PaletteChoice) -> Outcome {
        match choice {
            PaletteChoice::Command(command) => Outcome {
                command: Some(command),
                ..Outcome::default()
            },
            PaletteChoice::Conversation(session) => Outcome {
                resume: Some(session),
                ..Outcome::default()
            },
            PaletteChoice::Permission(choice) => {
                let Some(panel) = self
                    .state
                    .command_palette()
                    .and_then(crate::state::CommandPalette::permissions)
                else {
                    return Outcome::default();
                };
                if !panel
                    .choices()
                    .iter()
                    .any(|(current, _)| current == &choice)
                {
                    return Outcome::default();
                }
                let permission = self.state.activate_permission(&choice);
                Outcome {
                    permission,
                    command: matches!(choice, crate::state::permissions::PermissionChoice::Reload)
                        .then_some(Command::Permissions),
                    ..Outcome::default()
                }
            }
        }
    }
}

impl Workspace {
    /// Opens permission controls; loading and mutation work belongs to the application owner.
    pub fn open_permissions(&mut self) {
        self.state.open_permissions();
    }

    /// Whether a pending permission-control result still has a visible destination.
    pub fn permissions_open(&self) -> bool {
        self.state
            .command_palette()
            .and_then(crate::state::CommandPalette::permissions)
            .is_some()
    }

    /// Publishes an acknowledged permission view; late results cannot reopen a dismissed page.
    pub fn update_permissions(
        &mut self,
        view: Result<plexmaton_core::PermissionStateView, plexmaton_core::PermissionChangeError>,
        changed: Option<Result<(), plexmaton_core::PermissionChangeError>>,
    ) {
        self.state.update_permissions(view, changed);
    }
    /// Includes drafts temporarily displaced by edit/retry, so switching cannot silently lose them.
    pub fn has_unsent_input(&self) -> bool {
        self.state.has_unsent_input()
    }
    /// Opens the search surface before its owned loader supplies results.
    pub fn open_conversation_picker(&mut self) {
        self.state.open_conversation_picker();
    }

    /// Supplies bounded display choices; a result arriving after dismissal is ignored.
    pub fn set_conversation_choices(&mut self, entries: Vec<ConversationChoice>, limited: bool) {
        self.state.set_conversation_choices(entries, limited);
    }

    /// Updates only the affected picker, never the conversation or multi-agent Notices.
    pub fn set_conversation_picker_status(&mut self, status: ConversationPickerStatus) {
        self.state.session_picker_status(status);
    }

    /// Whether the user's permission to show picker work still exists.
    pub fn conversation_picker_open(&self) -> bool {
        self.state
            .command_palette()
            .is_some_and(|p| p.is_conversation_picker())
    }

    /// Returns focus without changing the selected durable session.
    pub fn close_conversation_picker(&mut self) {
        self.state.close_command_palette();
    }

    pub(super) fn activate_palette(&mut self) -> Outcome {
        let chosen = self.state.command_palette().and_then(|p| {
            if let Some(panel) = p.permissions() {
                let bounds = self.surfaces.get(SurfaceId::CommandPalette)?.bounds;
                let width = crate::surface::ContentInsets::for_surface(
                    SurfaceId::CommandPalette,
                    bounds.height,
                )
                .width(bounds.width);
                let choice = panel.choices().get(panel.selected())?.0.clone();
                if panel.is_reading() {
                    return Some(PaletteChoice::Permission(choice));
                }
                return crate::content_permissions::content(
                    panel,
                    &Palette::default(),
                    width,
                    bounds.height,
                )
                .choices
                .iter()
                .any(|(_, visible)| visible == &choice)
                .then_some(PaletteChoice::Permission(choice));
            }
            if p.is_conversation_picker()
                && !p.conversation_rows_visible(
                    self.surfaces.get(SurfaceId::CommandPalette)?.bounds.height,
                )
            {
                return None;
            }
            p.chosen_conversation()
                .map(PaletteChoice::Conversation)
                .or_else(|| p.chosen().map(PaletteChoice::Command))
        });
        chosen.map_or_else(Outcome::default, |choice| {
            self.activate_palette_choice(choice)
        })
    }

    fn palette_hit(&self, at: Point) -> Option<PaletteChoice> {
        let bounds = self.surfaces.get(SurfaceId::CommandPalette)?.bounds;
        let insets =
            crate::surface::ContentInsets::for_surface(SurfaceId::CommandPalette, bounds.height);
        if at.x < bounds.x + 1 + insets.sides
            || at.x >= bounds.right().saturating_sub(1 + insets.sides)
        {
            return None;
        }
        let palette = self.state.command_palette()?;
        if let Some(panel) = palette.permissions() {
            if panel.is_reading() {
                return (at.y == crate::render::permission_review::choice_row(bounds))
                    .then(|| {
                        panel
                            .choices()
                            .first()
                            .map(|(choice, _)| PaletteChoice::Permission(choice.clone()))
                    })
                    .flatten();
            }
            let row = usize::from(at.y.checked_sub(bounds.y + 1 + insets.vertical)?);
            return crate::content_permissions::content(
                panel,
                &Palette::default(),
                insets.width(bounds.width),
                bounds.height,
            )
            .choices
            .into_iter()
            .find(|(drawn, _)| *drawn == row)
            .map(|(_, choice)| PaletteChoice::Permission(choice));
        }
        let row = usize::from(
            at.y.checked_sub(bounds.y + 2 + insets.vertical + palette.choice_gap(bounds.height))?,
        );
        if at.y >= bounds.bottom().saturating_sub(1 + insets.vertical) {
            return None;
        }
        let window = palette.choice_window(bounds.height);
        let index = {
            if row >= window.len()
                || (palette.is_conversation_picker()
                    && !palette.conversation_rows_visible(bounds.height))
            {
                return None;
            }
            window.start + row
        };
        if index >= palette.match_count() {
            return None;
        }
        if let Some(sessions) = palette.conversations() {
            if !matches!(
                sessions.status,
                ConversationPickerStatus::Ready | ConversationPickerStatus::OpenFailed
            ) {
                return None;
            }
            sessions
                .matches(palette.filter().text())
                .get(index)
                .map(|entry| PaletteChoice::Conversation(entry.id.clone()))
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
                        self.activate_palette_choice(choice)
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
