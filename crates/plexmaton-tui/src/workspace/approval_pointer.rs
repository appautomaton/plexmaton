//! Approval buttons resolve the drawn card and retain its exact request until matching release.
use super::*;
use crate::{ApprovalIntent, Point, PointerIntent, SurfaceId};
use plexmaton_core::ApprovalDecision;

#[derive(Clone, Debug)]
pub(super) struct PressedApproval {
    target: ApprovalSubmission,
    at: Point,
}

impl Workspace {
    fn approval_hit(&self, surface: SurfaceId, at: Point) -> Option<ApprovalSubmission> {
        if surface != SurfaceId::Approval {
            return None;
        }
        let bounds = self.surfaces.get(surface)?.bounds;
        let viewport = self.surfaces.viewport(surface)?;
        let x = at.x.checked_sub(bounds.x + 1)?;
        let y = at.y.checked_sub(bounds.y + 1)?;
        if x >= viewport.content_width || y >= viewport.visible_rows {
            return None;
        }
        let row = viewport.offset + usize::from(y);
        let decision = if row + 2 == viewport.content_rows {
            ApprovalDecision::AllowOnce
        } else if row + 1 == viewport.content_rows {
            ApprovalDecision::Deny
        } else {
            return None;
        };
        if usize::from(x) >= crate::content::approval_option_label(decision).len() + 2 {
            return None;
        }
        let approval = self.state.approval()?;
        Some(ApprovalSubmission {
            to: approval.agent_id.clone(),
            approval_id: approval.approval_id.clone(),
            decision,
        })
    }

    pub(super) fn approval_pointer(&mut self, pointer: PointerIntent) -> Option<Outcome> {
        match pointer {
            PointerIntent::Press { surface, at } => {
                self.pressed_approval = None;
                let target = self.approval_hit(surface, at)?;
                self.state.focus_surface(&self.surfaces, surface);
                self.pressed_approval = Some(PressedApproval { target, at });
                Some(Outcome::default())
            }
            PointerIntent::Release { surface, at } => {
                let pressed = self.pressed_approval.take()?;
                let mut outcome = Outcome::default();
                if at == pressed.at
                    && self.approval_hit(surface, at).as_ref() == Some(&pressed.target)
                {
                    let direction = match pressed.target.decision {
                        ApprovalDecision::AllowOnce => Direction::Backward,
                        ApprovalDecision::Deny => Direction::Forward,
                    };
                    self.state.decide_approval(ApprovalIntent::Move(direction));
                    outcome.approval = self.state.decide_approval(ApprovalIntent::Decide);
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
