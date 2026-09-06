//! The Drawer's rows: choosing one by keyboard or pointer, and what leaves the workspace when it
//! is chosen. Listing, loading and permission work belong to the composition root.
use super::*;
use crate::{
    ConversationChoice, ConversationPickerStatus, ConversationRequest, Page, Point, PointerIntent,
    SurfaceId,
    state::{Drawer, permissions::PermissionPanel},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DrawerChoice {
    Page(Page),
    Conversation(ConversationRequest),
    Permission(crate::state::permissions::PermissionChoice),
}

impl Workspace {
    /// `↑↓` on a page that scrolls moves its viewport; everywhere else it moves the marker.
    pub(super) fn step_drawer(&mut self, direction: Direction) {
        let scrolls = self.state.drawer().is_some_and(|drawer| {
            drawer.configuration().is_some()
                || drawer
                    .permissions()
                    .is_some_and(PermissionPanel::is_reading)
        });
        if scrolls {
            self.state.scroll(
                &self.surfaces,
                &self.metrics,
                SurfaceId::Drawer,
                if direction == Direction::Forward {
                    crate::ScrollDirection::Down
                } else {
                    crate::ScrollDirection::Up
                },
            );
        } else {
            self.state
                .step_drawer_choice(direction == Direction::Forward);
        }
    }

    fn choose_drawer_row(&mut self, choice: DrawerChoice) -> Outcome {
        match choice {
            DrawerChoice::Page(page) => Outcome {
                page: Some(page),
                ..Outcome::default()
            },
            DrawerChoice::Conversation(request) => Outcome {
                conversation: Some(request),
                ..Outcome::default()
            },
            DrawerChoice::Permission(choice) => {
                let Some(panel) = self.state.drawer().and_then(Drawer::permissions) else {
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
                    page: matches!(choice, crate::state::permissions::PermissionChoice::Reload)
                        .then_some(Page::Permissions),
                    ..Outcome::default()
                }
            }
        }
    }
}

impl Workspace {
    /// Opens the Permissions page; loading and mutation work belongs to the application owner.
    pub fn open_permissions(&mut self) {
        self.state.open_permissions();
    }

    /// Whether a pending permission-control result still has a visible destination.
    pub fn permissions_open(&self) -> bool {
        self.state.drawer().and_then(Drawer::permissions).is_some()
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
    /// Opens the Conversations page before its owned loader supplies results.
    pub fn open_conversation_picker(&mut self) {
        self.state.open_conversation_picker();
    }

    /// Supplies bounded display choices; a result arriving after dismissal is ignored.
    pub fn set_conversation_choices(&mut self, entries: Vec<ConversationChoice>, limited: bool) {
        self.state.set_conversation_choices(entries, limited);
    }

    /// Updates only the affected page, never the conversation or multi-agent Notices.
    pub fn set_conversation_picker_status(&mut self, status: ConversationPickerStatus) {
        self.state.conversation_picker_status(status);
    }

    /// Whether the user's permission to show picker work still exists.
    pub fn conversation_picker_open(&self) -> bool {
        self.state
            .drawer()
            .is_some_and(Drawer::is_conversation_picker)
    }

    /// Closes the Drawer once a conversation is open, without changing the selected durable session.
    pub fn close_conversation_picker(&mut self) {
        self.state.close_drawer();
    }

    pub(super) fn choose_in_drawer(&mut self) -> Outcome {
        let chosen = self.state.drawer().and_then(|drawer| {
            let bounds = self.surfaces.get(SurfaceId::Drawer)?.bounds;
            if let Some(panel) = drawer.permissions() {
                let width =
                    crate::surface::ContentInsets::for_surface(SurfaceId::Drawer, bounds.height)
                        .width(bounds.width);
                let choice = panel.choices().get(panel.selected())?.0.clone();
                if panel.is_reading() {
                    return Some(DrawerChoice::Permission(choice));
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
                .then_some(DrawerChoice::Permission(choice));
            }
            if drawer.is_conversation_picker() {
                return drawer.chosen_request().map(DrawerChoice::Conversation);
            }
            drawer.chosen_page().map(DrawerChoice::Page)
        });
        chosen.map_or_else(Outcome::default, |choice| self.choose_drawer_row(choice))
    }

    fn drawer_hit(&self, at: Point) -> Option<DrawerChoice> {
        let bounds = self.surfaces.get(SurfaceId::Drawer)?.bounds;
        let insets = crate::surface::ContentInsets::for_surface(SurfaceId::Drawer, bounds.height);
        if at.x < bounds.x + 1 + insets.sides
            || at.x >= bounds.right().saturating_sub(1 + insets.sides)
        {
            return None;
        }
        let drawer = self.state.drawer()?;
        if let Some(panel) = drawer.permissions() {
            if panel.is_reading() {
                return (at.y == crate::render::permission_review::choice_row(bounds))
                    .then(|| {
                        panel
                            .choices()
                            .first()
                            .map(|(choice, _)| DrawerChoice::Permission(choice.clone()))
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
            .map(|(_, choice)| DrawerChoice::Permission(choice));
        }
        if drawer.configuration().is_some() {
            return None;
        }
        let row = usize::from(
            at.y.checked_sub(bounds.y + 2 + insets.vertical + drawer.choice_gap(bounds.height))?,
        );
        if at.y >= bounds.bottom().saturating_sub(1 + insets.vertical) {
            return None;
        }
        let window = drawer.choice_window(bounds.height);
        if row >= window.len() {
            return None;
        }
        let index = window.start + row;
        if index >= drawer.match_count() {
            return None;
        }
        if drawer.is_conversation_picker() {
            drawer
                .conversation_request_at(index)
                .map(DrawerChoice::Conversation)
        } else {
            drawer.pages().get(index).copied().map(DrawerChoice::Page)
        }
    }

    pub(super) fn drawer_pointer(&mut self, pointer: PointerIntent) -> Option<Outcome> {
        match pointer {
            PointerIntent::Press {
                surface: SurfaceId::Drawer,
                at,
            } => {
                self.pressed_drawer = self.drawer_hit(at).map(|choice| (choice, at));
                self.pressed_drawer.as_ref().map(|_| Outcome::default())
            }
            PointerIntent::Release {
                surface: SurfaceId::Drawer,
                at,
            } => {
                let (choice, original) = self.pressed_drawer.take()?;
                Some(
                    if at == original && self.drawer_hit(at) == Some(choice.clone()) {
                        self.choose_drawer_row(choice)
                    } else {
                        Outcome::default()
                    },
                )
            }
            PointerIntent::Drag { .. }
            | PointerIntent::Cancel { .. }
            | PointerIntent::Suspend { .. } => {
                self.pressed_drawer.take().map(|_| Outcome::default())
            }
            _ => None,
        }
    }
}
