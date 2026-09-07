//! Inspection retains request identity, while the tool entry remains the command's owner.
use super::{CopyRequest, ViewState};
use crate::SurfaceId;
use plexmaton_core::ToolDetail;

impl ViewState {
    pub(crate) fn approval_command(&self) -> Option<(&str, &str, u64)> {
        let approval = self.approval()?;
        let tool = self.agent(approval.agent_id)?.tool(approval.call_id)?;
        let ToolDetail::Command(command) = tool.presentation.invocation.as_ref()? else {
            return None;
        };
        Some((&command.source, &command.workspace_root, command.timeout_ms))
    }

    pub(crate) fn command_inspection_open(&self) -> bool {
        self.command_inspection
            .as_ref()
            .is_some_and(|target| self.approval().is_some_and(|view| target.matches(&view)))
            && self.approval_command().is_some()
    }

    pub(crate) fn open_command_inspection(&mut self) -> bool {
        if self.approval_command().is_none() {
            return false;
        }
        let Some(view) = self.approval() else {
            return false;
        };
        self.command_inspection = Some(view.target());
        self.scroll.reset_panel(SurfaceId::CommandInspection);
        self.focus.prefer(SurfaceId::CommandInspection);
        self.hover_entry(None);
        self.touch();
        true
    }

    pub(crate) fn close_command_inspection(&mut self) -> bool {
        if self.command_inspection.take().is_none() {
            return false;
        }
        self.focus.prefer(if self.approval().is_some() {
            SurfaceId::Approval
        } else {
            SurfaceId::Composer
        });
        self.hover_entry(None);
        self.touch();
        true
    }

    pub(super) fn reconcile_command_inspection(&mut self) {
        if self.command_inspection.is_some() && !self.command_inspection_open() {
            // Resolution may already have restored a worker or advanced the primary approval.
            let fallback = self
                .focus
                .preferred()
                .filter(|surface| {
                    !matches!(surface, SurfaceId::CommandInspection | SurfaceId::Drawer)
                })
                .unwrap_or(if self.approval().is_some() {
                    SurfaceId::Approval
                } else {
                    SurfaceId::Composer
                });
            if let Some(drawer) = &mut self.drawer {
                drawer.replace_return_focus(SurfaceId::CommandInspection, fallback);
            }
            if self.focus.preferred() == Some(SurfaceId::CommandInspection) {
                self.focus.prefer(fallback);
            }
            self.command_inspection = None;
            self.hover_entry(None);
        }
    }

    pub(crate) fn copy_inspected_command(&self) -> Option<CopyRequest> {
        if !self.command_inspection_open() {
            return None;
        }
        Some(CopyRequest {
            text: self.approval_command()?.0.to_owned(),
            entries: 1,
        })
    }
}
