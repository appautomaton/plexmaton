//! What an agent is asking the user, and what the workspace answers with.
//!
//! Split from the projection's own module because these read one thing: the queue of requests and
//! the region that answers the one in front of the user. The queue's ordering and the decision
//! surface's own state live beside this in `attention.rs` and `approval.rs`; what is here is the
//! workspace's view of both, which is what layout, chrome and routing consume.

use super::{ApprovalSubmission, ApprovalView, AttentionView, ViewState};
use crate::{
    content,
    intent::{ApprovalIntent, AttentionIntent},
    surface::{SurfaceId, SurfaceTree},
};

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

    /// Which queued request the user is on.
    #[must_use]
    pub fn attention_cursor(&self) -> usize {
        self.attention.cursor()
    }

    /// Returns queued attention items in arrival order.
    pub fn attention(&self) -> impl Iterator<Item = &AttentionView> {
        self.attention.iter()
    }

    /// Queued items the band still has something to say about.
    ///
    /// Main-agent requests stay in their conversation, including when the composer holds focus.
    /// Only future background requests belong here; the open background card is also excluded.
    pub fn attention_listed(&self) -> impl Iterator<Item = &AttentionView> {
        let open = self.approval();
        let primary = self.agents.primary().map(|agent| &agent.id);
        self.attention.iter().filter(move |item| {
            open.as_ref()
                .is_none_or(|approval| &item.id != approval.attention_id)
                && Some(&item.agent_id) != primary
        })
    }

    pub(crate) fn listed_attention_cursor(&self) -> Option<&plexmaton_core::AttentionId> {
        let current = self
            .attention
            .iter()
            .nth(self.attention.cursor())
            .map(|item| &item.id);
        self.attention_listed()
            .find(|item| Some(&item.id) == current)
            .or_else(|| self.attention_listed().next())
            .map(|item| &item.id)
    }

    /// How many the band would list, which is what decides whether it takes any rows.
    #[must_use]
    pub fn attention_listed_count(&self) -> usize {
        self.attention_listed().count()
    }

    /// How many of those are still unanswered, which is what the pill counts.
    ///
    /// The request the decision region is showing is not among them at either number: it is being
    /// answered, in front of the user, and a count that included it would send them looking for a
    /// second thing that does not exist.
    #[must_use]
    pub fn attention_listed_pending(&self) -> usize {
        self.attention_listed()
            .filter(|item| !item.acknowledged)
            .count()
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

    pub(crate) fn approval_in_primary(&self) -> bool {
        self.approval().is_some_and(|approval| {
            self.agents
                .primary()
                .is_some_and(|primary| &primary.id == approval.agent_id)
        })
    }

    /// Applies one user action to the Attention queue.
    ///
    /// Going to a request is the *only* thing in the workspace that lets a background agent change
    /// what the user is looking at, and it happens because the user pressed a key on it. Nothing on
    /// the producer path reaches here (ATT-1).
    pub fn attend(&mut self, _surfaces: &SurfaceTree, intent: AttentionIntent) {
        let changed = match intent {
            AttentionIntent::Move(direction) => {
                let ids: Vec<_> = self
                    .attention_listed()
                    .map(|item| item.id.clone())
                    .collect();
                self.attention.move_cursor(&ids, direction)
            }
            AttentionIntent::GoTo => {
                let Some(id) = self.listed_attention_cursor().cloned() else {
                    return;
                };
                self.attention.select(&id);
                let Some(target) = self.attention.acknowledge() else {
                    return;
                };
                // The agent may have left the roster; the acknowledgement still stands, because
                // the user did see it.
                let _selected = self.select_agent(&target.agent_id);
                // Going to a background agent opens its window, and the user asked to be taken
                // there, so the keyboard goes with them. The primary's conversation is already on
                // screen, so going to the primary is pointing at it.
                let destination = if self.agents.peeked().is_some() {
                    SurfaceId::Inspector
                } else {
                    SurfaceId::Transcript
                };
                if target.kind == plexmaton_core::AttentionKind::Approval {
                    self.approval.open(target.id, destination);
                    self.focus.prefer(SurfaceId::Approval);
                } else {
                    self.focus.prefer(destination);
                }
                true
            }
        };
        if changed {
            self.touch();
        }
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
