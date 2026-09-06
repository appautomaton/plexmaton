//! The composer menu's keys and pointer, and what leaves the workspace when a row is accepted.

use super::*;
use crate::{
    Command, CommandRun, CommandTarget, ConversationRequest, MenuIntent, PermissionRequest, Point,
    SurfaceId, state::MenuRow,
};

impl Workspace {
    /// Replaces the bounded skill completion catalog without loading any skill content.
    pub fn set_skills(&mut self, choices: Vec<crate::SkillChoice>) {
        self.state.set_skills(choices);
    }

    pub(super) fn apply_menu(&mut self, intent: MenuIntent) -> Outcome {
        match intent {
            MenuIntent::Step(direction) => {
                self.state.step_composer_menu(direction);
                Outcome::default()
            }
            // A permission review returns one layer before the menu closes (PER-7, DRW-3).
            MenuIntent::Close => {
                if !self.state.menu_permission_back() {
                    self.state.close_composer_menu();
                }
                Outcome::default()
            }
            MenuIntent::Complete => match self.state.menu_chosen() {
                // Completing a Command writes `/name ` and runs nothing (CMD-2).
                Some(MenuRow::Command(command)) => self.complete_command(command),
                other => self.accept_menu(other),
            },
            MenuIntent::Accept => self.accept_menu(None),
        }
    }

    fn complete_command(&mut self, command: Command) -> Outcome {
        self.state.complete_command(command);
        match command {
            Command::Resume => self.list_conversations(),
            Command::Permissions => self.list_session_permissions(),
            Command::New | Command::Compact => Outcome::default(),
        }
    }

    /// Asks the retained owner for the Session's view once, for `/permissions`' rows.
    fn list_session_permissions(&mut self) -> Outcome {
        if self.state.composer_menu().permissions.is_some() {
            return Outcome::default();
        }
        self.state.open_session_permissions();
        Outcome {
            permission: Some(PermissionRequest::Refresh),
            ..Outcome::default()
        }
    }

    /// Accepts `row`, or the chosen one, with the effect its listing states (ui-ux §input).
    pub(super) fn accept_menu(&mut self, row: Option<MenuRow>) -> Outcome {
        let Some(row) = row.or_else(|| self.state.menu_chosen()) else {
            return Outcome::default();
        };
        match row {
            MenuRow::Skill(name) => {
                self.state.accept_skill(Some(name));
                Outcome::default()
            }
            MenuRow::Command(Command::New) => {
                self.state.take_command_draft();
                Outcome {
                    conversation: Some(ConversationRequest::New),
                    ..Outcome::default()
                }
            }
            MenuRow::Command(command @ (Command::Resume | Command::Permissions)) => {
                self.complete_command(command)
            }
            MenuRow::Command(Command::Compact) => {
                let Some(agent) = self.state.primary_agent().map(|agent| agent.id.clone()) else {
                    return Outcome::default();
                };
                self.state.take_command_draft();
                Outcome {
                    command: Some(CommandRun {
                        command: Command::Compact,
                        target: CommandTarget { agent },
                    }),
                    ..Outcome::default()
                }
            }
            MenuRow::Conversation(id) => Outcome {
                conversation: self.state.conversation_request(&id),
                ..Outcome::default()
            },
            MenuRow::Permission(choice) => {
                let intent = self.state.activate_menu_permission(&choice);
                Outcome {
                    permission: intent.map(PermissionRequest::Change).or_else(|| {
                        matches!(choice, crate::state::permissions::PermissionChoice::Reload)
                            .then_some(PermissionRequest::Refresh)
                    }),
                    ..Outcome::default()
                }
            }
        }
    }

    /// Asks the composition root for the saved conversations once, for `/resume`'s rows.
    fn list_conversations(&mut self) -> Outcome {
        if self.state.conversation_picker_open() {
            return Outcome::default();
        }
        self.state.open_conversation_picker();
        Outcome {
            conversation: Some(ConversationRequest::List),
            ..Outcome::default()
        }
    }

    /// A whole draft that is a Command runs on `Enter` even with the menu dismissed (CMD-2).
    pub(super) fn submit_command(&mut self) -> Option<Outcome> {
        let command = self.state.exact_command()?;
        Some(self.accept_menu(Some(MenuRow::Command(command))))
    }

    pub(super) fn menu_hit(&self, at: Point) -> Option<MenuRow> {
        let bounds = self.surfaces.get(SurfaceId::ComposerMenu)?.bounds;
        if at.x <= bounds.x
            || at.x >= bounds.right().saturating_sub(1)
            || at.y <= bounds.y
            || at.y >= bounds.bottom().saturating_sub(1)
        {
            return None;
        }
        let heading = self
            .state
            .menu_heading(bounds.width.saturating_sub(2))
            .len();
        let row = usize::from(at.y.saturating_sub(bounds.y + 1)).checked_sub(heading)?;
        let input = self.state.composer();
        let visible = usize::from(bounds.height.saturating_sub(2))
            .saturating_sub(usize::from(self.state.menu_status().is_some()))
            .saturating_sub(heading);
        let rows = self.state.menu_rows();
        let window = self
            .state
            .composer_menu()
            .window(input.text(), input.cursor(), visible);
        if row >= window.len() {
            return None;
        }
        rows.get(window.start.saturating_add(row)).cloned()
    }
}
