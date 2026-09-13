//! The one button under the primary button (INV-11): what a matching release activates, on which
//! surface, at which cell. One slot for every surface with rows, so a press on one surface can
//! never activate a release on another, and a drag disarms it until the release consumes it.
use super::{drawer::DrawerChoice, *};
use crate::{
    ApprovalIntent, Point, PointerIntent, RetryAction, RetryTarget, SurfaceId, state::MenuRow,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Pressed {
    surface: SurfaceId,
    at: Point,
    target: PressTarget,
    focus: Option<SurfaceId>,
    /// A drag or a lost terminal disarms the press; the release still consumes it.
    armed: bool,
}

/// The row a press landed on, by identity rather than by position (INV-1).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PressTarget {
    Command {
        approval: crate::state::ApprovalTarget,
        action: crate::intent::CommandInspectionIntent,
    },
    Approval {
        approval: crate::state::ApprovalTarget,
        choice: crate::ApprovalChoice,
    },
    Retry {
        target: RetryTarget,
        action: RetryAction,
    },
    Drawer(DrawerChoice),
    DrawerRetract,
    Menu(MenuRow),
    Tree(crate::render::conversation_tree::Hit),
}

impl Workspace {
    /// The row under `at` on the surface the press names; every other surface has none.
    fn press_target(&self, surface: SurfaceId, at: Point) -> Option<PressTarget> {
        match surface {
            SurfaceId::ConversationTree => self.tree_hit(at).map(PressTarget::Tree),
            SurfaceId::ComposerMenu => self.menu_hit(at).map(PressTarget::Menu),
            SurfaceId::Drawer if self.drawer_retract_hit(at) => Some(PressTarget::DrawerRetract),
            SurfaceId::Drawer => self.drawer_hit(at).map(PressTarget::Drawer),
            SurfaceId::Approval => self
                .approval_hit(at)
                .map(|(approval, choice)| PressTarget::Approval { approval, choice })
                .or_else(|| {
                    self.command_summary_hit(at)
                        .map(|approval| PressTarget::Command {
                            approval,
                            action: crate::intent::CommandInspectionIntent::Open,
                        })
                }),
            SurfaceId::CommandInspection => self
                .command_control_hit(at)
                .map(|(approval, action)| PressTarget::Command { approval, action }),
            SurfaceId::Transcript => self
                .retry_hit(surface, at)
                .map(|(target, action)| PressTarget::Retry { target, action }),
            SurfaceId::Agents
            | SurfaceId::Inspector
            | SurfaceId::Composer
            | SurfaceId::Notices
            | SurfaceId::Attention
            | SurfaceId::QueuedInput
            | SurfaceId::Status => None,
        }
    }

    /// What the row does; the press and the release both resolved to it.
    fn activate(&mut self, target: PressTarget) -> Outcome {
        match target {
            PressTarget::Tree(hit) => self.activate_tree(&hit),
            PressTarget::Command { action, .. } => self.inspect_command(action),
            PressTarget::Menu(MenuRow::Effort(effort)) => {
                self.state.choose_effort(effort);
                Outcome::default()
            }
            PressTarget::Menu(row) => self.accept_menu(Some(row)),
            PressTarget::Drawer(choice) => self.choose_drawer_row(choice),
            PressTarget::DrawerRetract => {
                self.state.close_drawer();
                Outcome::default()
            }
            PressTarget::Approval { choice, .. } => {
                self.state.choose_approval(choice);
                Outcome {
                    approval: self.decide_visible_approval(ApprovalIntent::Decide),
                    ..Outcome::default()
                }
            }
            PressTarget::Retry { action, .. } => Outcome {
                retry: self.perform_retry_action(action),
                ..Outcome::default()
            },
        }
    }

    /// A button gesture from press to release, or nothing when the press landed on no row and
    /// the conversation's own pointer handling takes it.
    pub(super) fn button_pointer(&mut self, pointer: PointerIntent) -> Option<Outcome> {
        match pointer {
            PointerIntent::Press { surface, at } => {
                self.pressed = None;
                let target = self.press_target(surface, at)?;
                // The card takes the keyboard with the press, as a click into a region does.
                if surface == SurfaceId::Approval {
                    self.state.focus_surface(&self.surfaces, surface);
                }
                self.pressed = Some(Pressed {
                    surface,
                    at,
                    target,
                    focus: self.state.focused(&self.surfaces),
                    armed: true,
                });
                Some(Outcome::default())
            }
            PointerIntent::Release { surface, at } => {
                let pressed = self.pressed.take()?;
                let same_row = pressed.armed
                    && pressed.surface == surface
                    && pressed.at == at
                    && pressed.focus == self.state.focused(&self.surfaces)
                    && self.surfaces.hit_test(at) == Some(surface)
                    && self.press_target(surface, at).as_ref() == Some(&pressed.target);
                Some(if same_row {
                    self.activate(pressed.target)
                } else {
                    Outcome::default()
                })
            }
            PointerIntent::Drag { .. } | PointerIntent::Suspend { .. } => {
                self.pressed.as_mut()?.armed = false;
                Some(Outcome::default())
            }
            PointerIntent::Cancel { .. } => {
                self.pressed.take()?;
                Some(Outcome::default())
            }
        }
    }
}
