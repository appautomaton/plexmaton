//! What an agent is asking the user, and what the workspace answers with.
//!
//! Split from the projection's own module because these read one thing: the queue of requests and
//! the region that answers the one in front of the user. The queue's ordering and the decision
//! surface's own state live beside this in `attention.rs` and `approval.rs`; what is here is the
//! workspace's view of both, which is what layout, chrome and routing consume.

use super::{ApprovalSubmission, ApprovalView, AttentionView, ViewState, inner_width};
use crate::{
    content,
    intent::{ApprovalIntent, AttentionIntent},
    surface::{SurfaceId, SurfaceTree},
};

/// Rows the decision region spends on things other than the detail: the divider joining it to the
/// conversation above, the access line, and the two options.
const DECISION_CHROME_ROWS: u16 = 4;

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
    /// The request being answered in its own decision region is not one of them: it is already on
    /// screen, in front of the tool entry it concerns, and listing it again makes the band a
    /// second copy of the thing the user is looking at. It stays *queued* — that is where its
    /// resolution finds it (ATT-3) — and the pill still counts it.
    pub fn attention_listed(&self) -> impl Iterator<Item = &AttentionView> {
        let open = self
            .approval()
            .map(|approval| approval.attention_id.clone());
        self.attention
            .iter()
            .filter(move |item| Some(&item.id) != open.as_ref())
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

    /// Applies one user action to the Attention queue.
    ///
    /// Going to a request is the *only* thing in the workspace that lets a background agent change
    /// what the user is looking at, and it happens because the user pressed a key on it. Nothing on
    /// the producer path reaches here (ATT-1).
    pub fn attend(&mut self, _surfaces: &SurfaceTree, intent: AttentionIntent) {
        let changed = match intent {
            AttentionIntent::Move(direction) => self.attention.move_cursor(direction),
            AttentionIntent::GoTo => {
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
        match intent {
            ApprovalIntent::Move(direction) => {
                if self.approval.move_selection(direction) {
                    self.touch();
                }
                None
            }
            ApprovalIntent::ToggleDetail => {
                if self.approval.toggle_detail() {
                    self.touch();
                }
                None
            }
            ApprovalIntent::Decide => self.approval.submission(&self.attention),
        }
    }

    /// Rows the decision region asks for, or none when no request is open.
    ///
    /// Exact at every width: the region does not scroll, and the only part of it that can be more
    /// than one row is the disclosed detail, measured here by the wrapper that will paint it. The
    /// options are the last two rows of whatever comes back, so no width and no detail can move
    /// them.
    #[must_use]
    pub fn decision_rows(&self, width: u16) -> u16 {
        let Some(approval) = self.approval() else {
            return 0;
        };
        let detail = content::detail_rows(approval.detail, inner_width(width), approval.expanded);
        u16::try_from(detail.len())
            .unwrap_or(1)
            .saturating_add(DECISION_CHROME_ROWS)
    }
}
