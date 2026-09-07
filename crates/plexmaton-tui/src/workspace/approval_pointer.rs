//! Approval controls share painted identity, row geometry and scope guards across input routes.
use super::*;
use crate::{ApprovalChoice, ApprovalIntent, Point, SurfaceId};

#[derive(Debug)]
pub(super) struct PaintedApproval {
    target: crate::state::ApprovalTarget,
    stage: crate::ApprovalStage,
    offer: Option<(
        plexmaton_core::PermissionOfferId,
        plexmaton_core::PermissionScopes,
    )>,
}

impl Workspace {
    pub(super) fn approval_frame(&self) -> Option<PaintedApproval> {
        self.surfaces.get(SurfaceId::Approval)?;
        let view = self.state.approval()?;
        Some(PaintedApproval {
            target: view.target(),
            stage: view.stage,
            offer: view.remember.map(|offer| (offer.id, offer.scopes)),
        })
    }

    fn approval_is_painted(&self) -> bool {
        self.painted_approval.as_ref().is_some_and(|painted| {
            self.state.approval().is_some_and(|view| {
                painted.target.matches(&view)
                    && painted.stage == view.stage
                    && painted.offer == view.remember.map(|offer| (offer.id, offer.scopes))
            })
        })
    }

    pub(super) fn decide_visible_approval(
        &mut self,
        intent: ApprovalIntent,
    ) -> Option<ApprovalSubmission> {
        if self.state.drawer().is_some()
            || self.state.command_inspection_open()
            || (!matches!(intent, ApprovalIntent::Move(_)) && !self.approval_is_painted())
        {
            return None;
        }
        let view = self.state.approval()?;
        let viewport = self.surfaces.viewport(SurfaceId::Approval)?;
        let selected = if let ApprovalIntent::Shortcut(number) = intent {
            let choice = *view.choices().get(usize::from(number.checked_sub(1)?))?;
            let visible = crate::content::approval_choice_rows(
                &self.state,
                viewport.content_width,
                viewport.visible_rows,
            )
            .into_iter()
            .any(|(row, candidate)| {
                candidate == choice
                    && row >= viewport.offset
                    && row < viewport.offset + usize::from(viewport.visible_rows)
            });
            if !visible {
                return None;
            }
            choice
        } else {
            view.selected
        };
        if matches!(intent, ApprovalIntent::Decide | ApprovalIntent::Shortcut(_))
            && view.stage == crate::ApprovalStage::Remember
            && selected != ApprovalChoice::Back
            && !crate::content::approval_scope_fits(
                &self.state,
                viewport.content_width,
                viewport.visible_rows,
            )
        {
            return None;
        }
        self.state.decide_approval(intent)
    }

    /// The choice row under `at` on the card, with the approval it belongs to (APV-4).
    pub(super) fn approval_hit(
        &self,
        at: Point,
    ) -> Option<(crate::state::ApprovalTarget, ApprovalChoice)> {
        if self.state.drawer().is_some()
            || self.state.command_inspection_open()
            || !self.approval_is_painted()
        {
            return None;
        }
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
        if usize::from(x) >= unicode_width::UnicodeWidthStr::width(choice.label()) + 5 {
            return None;
        }
        Some((self.state.approval()?.target(), choice))
    }
}

impl Workspace {
    pub(super) fn inspect_command(
        &mut self,
        action: crate::intent::CommandInspectionIntent,
    ) -> Outcome {
        use crate::intent::CommandInspectionIntent;
        if self.state.drawer().is_some() {
            return Outcome::default();
        }
        match action {
            CommandInspectionIntent::Open => {
                self.state.open_command_inspection();
            }
            CommandInspectionIntent::Close => {
                self.state.close_command_inspection();
            }
            CommandInspectionIntent::Copy => {
                return Outcome {
                    copied: self.state.copy_inspected_command(),
                    ..Outcome::default()
                };
            }
        }
        Outcome::default()
    }

    pub(super) fn command_summary_hit(&self, at: Point) -> Option<crate::state::ApprovalTarget> {
        if self.state.drawer().is_some()
            || self.state.command_inspection_open()
            || !self.approval_is_painted()
        {
            return None;
        }
        self.state.approval_command()?;
        let view = self.state.approval()?;
        if view.stage != crate::ApprovalStage::Review {
            return None;
        }
        let surface = self.surfaces.get(SurfaceId::Approval)?;
        let viewport = surface.viewport?;
        let insets =
            crate::surface::ContentInsets::for_surface(SurfaceId::Approval, surface.bounds.height);
        let row = surface.bounds.y + 1 + insets.vertical;
        let left = surface.bounds.x + 1 + insets.sides;
        (viewport.offset == 0
            && viewport.visible_rows > view.choices().len() as u16
            && at.y == row
            && at.x >= left
            && at.x < left + viewport.content_width)
            .then(|| view.target())
    }

    pub(super) fn command_control_hit(
        &self,
        at: Point,
    ) -> Option<(
        crate::state::ApprovalTarget,
        crate::intent::CommandInspectionIntent,
    )> {
        if self.state.drawer().is_some() || !self.state.command_inspection_open() {
            return None;
        }
        let bounds = self.surfaces.get(SurfaceId::CommandInspection)?.bounds;
        let [copy, close] = crate::layout::command_inspection_controls(bounds);
        let action = if copy.contains((at.x, at.y).into()) {
            crate::intent::CommandInspectionIntent::Copy
        } else if close.contains((at.x, at.y).into()) {
            crate::intent::CommandInspectionIntent::Close
        } else {
            return None;
        };
        Some((self.state.approval()?.target(), action))
    }
}
