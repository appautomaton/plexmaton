//! Pointer decisions use the same choice rows and insets as the visible approval card.
use super::*;
use crate::{ApprovalChoice, ApprovalIntent, Point, PointerIntent, SurfaceId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PressedApproval {
    approval: plexmaton_core::ApprovalId,
    choice: ApprovalChoice,
    at: Point,
}

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

    fn approval_hit(&self, surface: SurfaceId, at: Point) -> Option<PressedApproval> {
        if surface != SurfaceId::Approval {
            return None;
        }
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
        Some(PressedApproval {
            approval: self.state.approval()?.approval_id.clone(),
            choice,
            at,
        })
    }

    pub(super) fn approval_pointer(&mut self, pointer: PointerIntent) -> Option<Outcome> {
        match pointer {
            PointerIntent::Press { surface, at } => {
                self.pressed_approval = None;
                let target = self.approval_hit(surface, at)?;
                self.state.focus_surface(&self.surfaces, surface);
                self.pressed_approval = Some(target);
                Some(Outcome::default())
            }
            PointerIntent::Release { surface, at } => {
                let pressed = self.pressed_approval.take()?;
                let mut outcome = Outcome::default();
                if self.approval_hit(surface, at).as_ref() == Some(&pressed) {
                    self.state.choose_approval(pressed.choice);
                    outcome.approval = self.decide_visible_approval(ApprovalIntent::Decide);
                }
                Some(outcome)
            }
            PointerIntent::Drag { .. }
            | PointerIntent::Suspend { .. }
            | PointerIntent::Cancel { .. } => {
                self.pressed_approval.take().map(|_| Outcome::default())
            }
        }
    }
}
