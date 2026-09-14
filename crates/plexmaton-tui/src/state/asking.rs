//! What an agent is asking the user, and what the workspace answers with.
//!
//! Split from the projection's own module because these read one thing: the queue of requests and
//! the region that answers the one in front of the user. The queue's ordering and the decision
//! surface's own state live beside this in `attention.rs` and `approval.rs`; what is here is the
//! workspace's view of both, which is what layout, chrome and routing consume.

use super::{ApprovalSubmission, ApprovalView, AttentionView, ViewState};
use crate::{content, intent::ApprovalIntent, surface::SurfaceId};

impl ViewState {
    /// Number of background requests awaiting attention.
    #[must_use]
    pub fn attention_count(&self) -> usize {
        self.attention.len()
    }

    /// Number the user has not been to yet, which is what reads as action required.
    #[must_use]
    pub fn attention_pending(&self) -> usize {
        self.attention.pending()
    }

    /// Returns queued attention items in arrival order.
    pub fn attention(&self) -> impl Iterator<Item = &AttentionView> {
        self.attention.iter()
    }

    /// What this agent is waiting on the user for, if anything.
    ///
    /// The roster asks this per row: a request is announced beside the agent that raised it, which
    /// is the only place the user can act on it. Excluding the primary keeps ATT-1 — its approvals
    /// belong to its own conversation, which is the screen the user is already looking at.
    ///
    /// An acknowledged request is still here, because ATT-3 says being seen is not being answered
    /// and only the owning loop resolves one. Dropping it would leave a user who looked at a card
    /// and closed it with no way back to it; what changes when it is seen is how loudly the row
    /// says so, not whether it says so.
    ///
    /// An approval outranks a clarification, because one agent is blocked and the other is not.
    /// Within a kind it is arrival order, so the oldest thing waiting is the one answered first.
    pub(crate) fn agent_request(&self, agent: &plexmaton_core::AgentId) -> Option<&AttentionView> {
        let primary = self.agents.primary().map(|agent| &agent.id);
        let mut outstanding = self
            .attention
            .iter()
            .filter(|item| &item.agent_id == agent && Some(agent) != primary);
        let mut first = None;
        for item in &mut outstanding {
            if item.kind() == plexmaton_core::AttentionKind::Approval {
                return Some(item);
            }
            first.get_or_insert(item);
        }
        first
    }

    /// The user-opened approval presentation, if its loop-owned request is still pending.
    #[must_use]
    pub fn approval(&self) -> Option<ApprovalView<'_>> {
        self.approval.view(&self.attention)
    }

    /// One inline card drains the primary's existing queue; a later arrival never replaces it.
    pub(super) fn open_next_primary_approval(&mut self) -> bool {
        if self.approval.has_primary() {
            return false;
        }
        let Some(primary) = self.agents.primary() else {
            return false;
        };
        let next = self
            .attention
            .iter()
            .find(|item| {
                item.agent_id == primary.id
                    && matches!(
                        item.request,
                        plexmaton_core::AttentionRequest::Approval { .. }
                    )
            })
            .map(|item| item.id.clone());
        let Some(next) = next else {
            return false;
        };
        if self.approval.open(next, SurfaceId::Composer) {
            if self.agents.peeked().is_none() {
                self.focus.prefer(SurfaceId::Approval);
            }
            return true;
        }
        false
    }

    /// Going to an agent is going to what it is asking, when it is asking something.
    ///
    /// ATT-2 made this a keypress on a band listing requests. The roster is that list now — a
    /// request is announced on its agent's row — so entering the agent *is* going to the request,
    /// and it stays the user's move: no producer path reaches here. ATT-3 still holds, because
    /// this marks the request seen and leaves it queued; only the owning loop resolves it.
    pub(super) fn visit_request(&mut self) -> bool {
        let Some(agent) = self.agents.peeked().map(|agent| agent.id.clone()) else {
            return false;
        };
        let Some(id) = self.agent_request(&agent).map(|item| item.id.clone()) else {
            return false;
        };
        self.attention.select(&id);
        let Some(target) = self.attention.acknowledge() else {
            return false;
        };
        // A clarification is answered by reading the conversation it was raised in, which entering
        // the agent has already opened. An approval needs its card, in that same conversation.
        if target.kind == plexmaton_core::AttentionKind::Approval {
            self.approval.open(target.id, SurfaceId::Inspector);
            self.focus.prefer(SurfaceId::Approval);
        }
        true
    }

    pub(crate) fn approval_in_primary(&self) -> bool {
        self.approval().is_some_and(|approval| {
            self.agents
                .primary()
                .is_some_and(|primary| &primary.id == approval.agent_id)
        })
    }

    /// Moves within or answers the user-opened approval surface.
    pub fn decide_approval(&mut self, intent: ApprovalIntent) -> Option<ApprovalSubmission> {
        self.hover_entry(None);
        match intent {
            ApprovalIntent::Shortcut(number) => {
                let index = usize::from(number.checked_sub(1)?);
                let choice = *self.approval()?.choices().get(index)?;
                self.choose_approval(choice);
                let submission = self.approval.submission(&self.attention);
                self.touch();
                submission
            }
            ApprovalIntent::Move(direction) => {
                if self.approval.move_selection(&self.attention, direction) {
                    self.touch();
                }
                None
            }
            ApprovalIntent::ToggleDetail => {
                if self.open_command_inspection() {
                    return None;
                }
                if self.approval.toggle_detail() {
                    self.touch();
                }
                None
            }
            ApprovalIntent::Decide => {
                let submission = self.approval.submission(&self.attention);
                self.touch();
                submission
            }
        }
    }

    pub(crate) fn choose_approval(&mut self, choice: super::ApprovalChoice) {
        if self.approval.choose(&self.attention, choice) {
            self.touch();
        }
    }

    /// Applies correlated producer feedback without deriving or granting any permission in the UI.
    pub fn report_approval_refusal(
        &mut self,
        agent: &plexmaton_core::AgentId,
        approval: &plexmaton_core::ApprovalId,
        feedback: super::ApprovalFeedback,
        current_offer: Option<plexmaton_core::RememberPermissionOffer>,
    ) {
        let Some(id) = self.attention.iter().find(|item| &item.agent_id == agent && matches!(&item.request, plexmaton_core::AttentionRequest::Approval { approval_id, .. } if approval_id == approval)).map(|item| item.id.clone()) else { return; };
        self.attention.update_offer(&id, current_offer);
        self.approval.refused(&id, feedback);
        self.touch();
    }

    /// Rows the decision region asks for, or none when no request is open.
    ///
    /// Measured with the same operation, policy reason, scope and padding that will be drawn.
    /// Small terminals trim secondary content before the current stage's actionable choices.
    #[must_use]
    pub fn decision_rows(&self, width: u16) -> u16 {
        if self.approval().is_none() {
            return 0;
        }
        let insets = crate::surface::ContentInsets::for_surface(SurfaceId::Approval, u16::MAX);
        let rows = content::approval(
            self,
            &crate::theme::Palette::default(),
            insets.width(width),
            u16::MAX,
        )
        .len();
        u16::try_from(rows)
            .unwrap_or(u16::MAX)
            .saturating_add(1 + insets.vertical * 2)
    }
}
