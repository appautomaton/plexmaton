//! Pointer decisions use the same choice rows and insets as the visible approval card.
use super::*;
use crate::{ApprovalChoice, ApprovalIntent, Point, SurfaceId};

impl Workspace {
    pub(super) fn decide_visible_approval(
        &mut self,
        intent: ApprovalIntent,
    ) -> Option<ApprovalSubmission> {
        if matches!(intent, ApprovalIntent::Decide)
            && self.state.approval().is_some_and(|view| {
                view.stage == crate::ApprovalStage::Remember
                    && view.selected != ApprovalChoice::Back
            })
            && !self
                .surfaces
                .viewport(SurfaceId::Approval)
                .is_some_and(|viewport| {
                    crate::content::approval_scope_fits(
                        &self.state,
                        viewport.content_width,
                        viewport.visible_rows,
                    )
                })
        {
            return None;
        }
        self.state.decide_approval(intent)
    }

    /// The choice row under `at` on the card, with the approval it belongs to (APV-4).
    pub(super) fn approval_hit(
        &self,
        at: Point,
    ) -> Option<(plexmaton_core::ApprovalId, ApprovalChoice)> {
        let surface = SurfaceId::Approval;
        let bounds = self.surfaces.get(surface)?.bounds;
        let viewport = self.surfaces.viewport(surface)?;
        let insets = crate::surface::ContentInsets::for_surface(surface, bounds.height);
        let x = at.x.checked_sub(bounds.x + 1 + insets.sides)?;
        let y = at.y.checked_sub(bounds.y + 1 + insets.vertical)?;
        if x >= viewport.content_width || y >= viewport.visible_rows {
            return None;
        }
        let rows = crate::content::approval_choice_rows(
            &self.state,
            viewport.content_width,
            viewport.visible_rows,
        );
        let (_, choice) = rows
            .into_iter()
            .find(|(row, _)| *row == viewport.offset + usize::from(y))?;
        if usize::from(x) >= unicode_width::UnicodeWidthStr::width(choice.label()) + 2 {
            return None;
        }
        Some((self.state.approval()?.approval_id.clone(), choice))
    }
}
