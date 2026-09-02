//! Presentation state for the approval surface.
//!
//! The loop owns whether a call is pending. This module remembers only which queued request the
//! user opened, which answer is highlighted, and where focus returns when the surface closes.

use plexmaton_core::{
    AgentId, ApprovalDecision, ApprovalId, AttentionId, AttentionRequest, ToolCallId,
    ToolCapability,
};

use super::{AttentionView, attention::AttentionQueue};
use crate::{intent::Direction, surface::SurfaceId};

/// The approval request currently presented to the user.
#[derive(Clone, Copy, Debug)]
pub struct ApprovalView<'a> {
    pub attention_id: &'a AttentionId,
    pub agent_id: &'a AgentId,
    pub approval_id: &'a ApprovalId,
    pub call_id: &'a ToolCallId,
    pub tool: &'a str,
    pub capabilities: &'a [ToolCapability],
    pub detail: &'a str,
    pub selected: ApprovalDecision,
}

/// A decision leaving the presentation boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalSubmission {
    /// Agent whose loop owns the pending call.
    pub to: AgentId,
    /// Exact pending request being answered (APV-4).
    pub approval_id: ApprovalId,
    /// The user's bounded decision vocabulary.
    pub decision: ApprovalDecision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OpenApproval {
    attention_id: AttentionId,
    selected: ApprovalDecision,
    return_focus: SurfaceId,
}

/// Ephemeral presentation state. No admitted call, waiter, sender, or policy decision lives here.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ApprovalSurface {
    open: Option<OpenApproval>,
}

impl ApprovalSurface {
    /// Opens one queued request. Deny is highlighted first so an accidental Enter cannot grant
    /// authority.
    pub(super) fn open(&mut self, attention_id: AttentionId, return_focus: SurfaceId) -> bool {
        if self
            .open
            .as_ref()
            .is_some_and(|open| open.attention_id == attention_id)
        {
            return false;
        }
        self.open = Some(OpenApproval {
            attention_id,
            selected: ApprovalDecision::Deny,
            return_focus,
        });
        true
    }

    pub(super) fn view<'a>(&self, queue: &'a AttentionQueue) -> Option<ApprovalView<'a>> {
        let open = self.open.as_ref()?;
        let AttentionView {
            id,
            agent_id,
            request:
                AttentionRequest::Approval {
                    approval_id,
                    call_id,
                    tool,
                    capabilities,
                    detail,
                },
            ..
        } = queue.get(&open.attention_id)?
        else {
            return None;
        };
        Some(ApprovalView {
            attention_id: id,
            agent_id,
            approval_id,
            call_id,
            tool,
            capabilities,
            detail,
            selected: open.selected,
        })
    }

    pub(super) fn move_selection(&mut self, direction: Direction) -> bool {
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        let next = match direction {
            Direction::Backward => ApprovalDecision::AllowOnce,
            Direction::Forward => ApprovalDecision::Deny,
        };
        if open.selected == next {
            return false;
        }
        open.selected = next;
        true
    }

    pub(super) fn submission(&self, queue: &AttentionQueue) -> Option<ApprovalSubmission> {
        let view = self.view(queue)?;
        Some(ApprovalSubmission {
            to: view.agent_id.clone(),
            approval_id: view.approval_id.clone(),
            decision: view.selected,
        })
    }

    /// Closes the presentation without answering the request and returns where focus belonged.
    pub(super) fn dismiss(&mut self) -> Option<SurfaceId> {
        self.open.take().map(|open| open.return_focus)
    }

    /// Closes the presentation only when the loop resolved the request it was showing.
    pub(super) fn resolved(&mut self, attention_id: &AttentionId) -> Option<SurfaceId> {
        if self
            .open
            .as_ref()
            .is_some_and(|open| &open.attention_id == attention_id)
        {
            return self.dismiss();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, ApprovalDecision, ApprovalId, AttentionId, AttentionRequest, ToolCallId,
        ToolCapability,
    };

    use super::ApprovalSurface;
    use crate::{
        AttentionView, intent::Direction, state::attention::AttentionQueue, surface::SurfaceId,
    };

    fn queue() -> AttentionQueue {
        let mut queue = AttentionQueue::default();
        queue.request(AttentionView {
            id: AttentionId::new("attention-1").unwrap_or_else(|error| panic!("fixture: {error}")),
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            request: AttentionRequest::Approval {
                approval_id: ApprovalId::new("approval-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new("call-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "edit".to_owned(),
                capabilities: vec![ToolCapability::FileWrite],
                detail: "change src/lib.rs".to_owned(),
            },
            acknowledged: true,
        });
        queue
    }

    /// APV-4: the presentation returns only the identity the loop supplied and one typed answer.
    #[test]
    fn a_decision_echoes_the_open_request_and_cannot_recompute_policy() {
        let queue = queue();
        let mut surface = ApprovalSurface::default();
        surface.open(
            AttentionId::new("attention-1").unwrap_or_else(|error| panic!("fixture: {error}")),
            SurfaceId::Inspector,
        );

        let denied = surface
            .submission(&queue)
            .unwrap_or_else(|| panic!("fixture is an approval"));
        assert_eq!(denied.approval_id.as_str(), "approval-1");
        assert_eq!(denied.decision, ApprovalDecision::Deny);

        assert!(surface.move_selection(Direction::Backward));
        assert_eq!(
            surface
                .submission(&queue)
                .unwrap_or_else(|| panic!("fixture is an approval"))
                .decision,
            ApprovalDecision::AllowOnce
        );
    }

    #[test]
    fn dismissal_returns_focus_without_resolving_the_queue() {
        let queue = queue();
        let mut surface = ApprovalSurface::default();
        surface.open(
            AttentionId::new("attention-1").unwrap_or_else(|error| panic!("fixture: {error}")),
            SurfaceId::Inspector,
        );

        assert_eq!(surface.dismiss(), Some(SurfaceId::Inspector));
        assert_eq!(queue.len(), 1, "Escape does not answer the pending request");
    }
}
