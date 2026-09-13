//! INV-3: actual mouse movement shares the focused menu's choice; it never activates or focuses.
use super::{Workspace, drawer::DrawerChoice};
use crate::{Point, SurfaceId};

impl Workspace {
    pub(super) fn hover_choice(&mut self, surface: Option<SurfaceId>, at: Point) {
        if self.surfaces.hit_test(at) != surface {
            return;
        }
        let focus = self.state.focused(&self.surfaces);
        match surface {
            Some(SurfaceId::ConversationTree) if focus == surface => {
                if let Some(hit) = self.tree_hit(at) {
                    self.hover_tree(&hit);
                }
            }
            Some(SurfaceId::Approval) if focus == surface => {
                if let Some((_, choice)) = self.approval_hit(at) {
                    self.state.choose_approval(choice);
                }
            }
            Some(SurfaceId::Drawer) if focus == surface => match self.drawer_hit(at) {
                Some(DrawerChoice::Page(page)) => self.state.hover_drawer_page(page),
                Some(DrawerChoice::Permission(choice)) => {
                    self.state.hover_drawer_permission(&choice);
                }
                None => {}
            },
            Some(SurfaceId::ComposerMenu)
                if focus == Some(SurfaceId::Composer) && self.state.drawer().is_none() =>
            {
                if let Some(row) = self.menu_hit(at) {
                    self.state.choose_menu_row(row);
                }
            }
            _ => {}
        }
    }
}
