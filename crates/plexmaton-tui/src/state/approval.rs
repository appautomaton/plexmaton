//! Presentation of producer-owned approvals, including the reviewed remembered-permission journey.

use super::{AttentionView, attention::AttentionQueue};
use crate::{intent::Direction, surface::SurfaceId};
use plexmaton_core::{
    AgentId, ApprovalDecision, ApprovalId, AttentionId, AttentionRequest, PermissionOfferId,
    PermissionScope, PermissionScopes, RememberPermissionOffer, ToolCallId, ToolCapability,
};

/// A visible action in the approval card; intermediate choices never leave as permission decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalChoice {
    AllowOnce,
    Remember,
    Deny,
    ThisSession,
    ThisProject,
    Back,
}

impl ApprovalChoice {
    /// Label shared by rendering and pointer hit testing.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::AllowOnce => "Allow once",
            Self::Remember => "Allow and remember…",
            Self::Deny => "Deny",
            Self::ThisSession => "This Session",
            Self::ThisProject => "This Project",
            Self::Back => "Back",
        }
    }
}

/// Visible phase, derived from the card's explicit journey state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalStage {
    Review,
    Remember,
    Submitting,
}

/// Typed producer feedback translated at the application boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalFeedback {
    NotPending,
    Preparing,
    PolicyChanged,
    Capacity,
    Unavailable,
    Ineffective,
    NotFound,
}

impl ApprovalFeedback {
    /// Local copy; it carries no policy authority.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::NotPending => "This request is no longer pending.",
            Self::Preparing => "Applying the decision. Waiting for confirmation…",
            Self::PolicyChanged => "Permissions changed. Nothing ran; review the updated choices.",
            Self::Capacity => "Permission capacity reached. Nothing ran; this request still waits.",
            Self::Unavailable => {
                "Permission preparation failed. Nothing ran; this request still waits."
            }
            Self::Ineffective => {
                "An explicit rule prevents remembering this operation. Nothing ran."
            }
            Self::NotFound => "The permission is no longer active. Nothing ran.",
        }
    }
}

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
    pub reason: plexmaton_core::ApprovalReason,
    pub remember: Option<&'a RememberPermissionOffer>,
    pub selected: ApprovalChoice,
    pub stage: ApprovalStage,
    pub feedback: Option<ApprovalFeedback>,
    pub expanded: bool,
}

/// A pointer/inspection target pins the complete request, including coalesced queue replacements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ApprovalTarget {
    approval: ApprovalId,
    attention: AttentionId,
    agent: AgentId,
    call: ToolCallId,
}

impl ApprovalTarget {
    pub(crate) fn matches(&self, view: &ApprovalView<'_>) -> bool {
        view.approval_id == &self.approval
            && view.attention_id == &self.attention
            && view.agent_id == &self.agent
            && view.call_id == &self.call
    }
}

impl ApprovalView<'_> {
    pub(crate) fn target(&self) -> ApprovalTarget {
        ApprovalTarget {
            approval: self.approval_id.clone(),
            attention: self.attention_id.clone(),
            agent: self.agent_id.clone(),
            call: self.call_id.clone(),
        }
    }

    /// Only actions the producer's current offer can support.
    #[must_use]
    pub fn choices(&self) -> &'static [ApprovalChoice] {
        use ApprovalChoice::{AllowOnce, Back, Deny, Remember, ThisProject, ThisSession};
        match self.stage {
            ApprovalStage::Review if self.remember.is_some() => &[AllowOnce, Remember, Deny],
            ApprovalStage::Review => &[AllowOnce, Deny],
            ApprovalStage::Remember
                if self
                    .remember
                    .is_some_and(|offer| offer.scopes == PermissionScopes::SessionAndProject) =>
            {
                &[ThisSession, ThisProject, Back]
            }
            ApprovalStage::Remember => &[ThisSession, Back],
            ApprovalStage::Submitting => &[],
        }
    }
}

/// A decision leaving the presentation boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalSubmission {
    pub to: AgentId,
    pub approval_id: ApprovalId,
    pub decision: ApprovalDecision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Journey {
    Review(ApprovalChoice),
    Remember {
        selected: ApprovalChoice,
        offer: PermissionOfferId,
    },
    Submitting(ApprovalDecision),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OpenApproval {
    attention_id: AttentionId,
    journey: Journey,
    return_focus: SurfaceId,
    expanded: bool,
    feedback: Option<ApprovalFeedback>,
}

/// Each conversation retains its open card while a background card is visited (ATT-1, PER-5).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ApprovalSurface {
    primary: Option<OpenApproval>,
    background: Option<OpenApproval>,
}

impl ApprovalSurface {
    pub(super) fn open(&mut self, attention_id: AttentionId, return_focus: SurfaceId) -> bool {
        let slot = if return_focus == SurfaceId::Composer {
            &mut self.primary
        } else {
            &mut self.background
        };
        if slot
            .as_ref()
            .is_some_and(|open| open.attention_id == attention_id)
        {
            return false;
        }
        *slot = Some(OpenApproval {
            attention_id,
            journey: Journey::Review(ApprovalChoice::Deny),
            return_focus,
            expanded: false,
            feedback: None,
        });
        true
    }

    pub(super) fn has_primary(&self) -> bool {
        self.primary.is_some()
    }
    fn active(&self) -> Option<&OpenApproval> {
        self.background.as_ref().or(self.primary.as_ref())
    }
    fn active_mut(&mut self) -> Option<&mut OpenApproval> {
        self.background.as_mut().or(self.primary.as_mut())
    }

    pub(super) fn view<'a>(&self, queue: &'a AttentionQueue) -> Option<ApprovalView<'a>> {
        let open = self.active()?;
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
                    reason,
                    remember,
                },
            ..
        } = queue.get(&open.attention_id)?
        else {
            return None;
        };
        let (stage, selected) = match open.journey {
            Journey::Review(selected) => (ApprovalStage::Review, selected),
            Journey::Remember { selected, .. } => (ApprovalStage::Remember, selected),
            Journey::Submitting(decision) => (
                ApprovalStage::Submitting,
                match decision {
                    ApprovalDecision::AllowOnce => ApprovalChoice::AllowOnce,
                    ApprovalDecision::Deny => ApprovalChoice::Deny,
                    ApprovalDecision::AllowAndRemember {
                        scope: PermissionScope::Session,
                        ..
                    } => ApprovalChoice::ThisSession,
                    ApprovalDecision::AllowAndRemember {
                        scope: PermissionScope::Project,
                        ..
                    } => ApprovalChoice::ThisProject,
                },
            ),
        };
        Some(ApprovalView {
            attention_id: id,
            agent_id,
            approval_id,
            call_id,
            tool,
            capabilities,
            detail,
            reason: *reason,
            remember: remember.as_ref(),
            selected,
            stage,
            feedback: open.feedback,
            expanded: open.expanded,
        })
    }

    pub(super) fn toggle_detail(&mut self) -> bool {
        let Some(open) = self.active_mut() else {
            return false;
        };
        open.expanded = !open.expanded;
        true
    }

    pub(super) fn choose(&mut self, queue: &AttentionQueue, choice: ApprovalChoice) -> bool {
        if self
            .view(queue)
            .is_none_or(|view| !view.choices().contains(&choice))
        {
            return false;
        }
        let Some(open) = self.active_mut() else {
            return false;
        };
        match &mut open.journey {
            Journey::Review(selected) | Journey::Remember { selected, .. } => {
                let changed = *selected != choice;
                *selected = choice;
                changed
            }
            Journey::Submitting(_) => false,
        }
    }

    pub(super) fn move_selection(&mut self, queue: &AttentionQueue, direction: Direction) -> bool {
        let Some(view) = self.view(queue) else {
            return false;
        };
        let choices = view.choices();
        let Some(index) = choices.iter().position(|choice| *choice == view.selected) else {
            return false;
        };
        let next = match direction {
            Direction::Backward => index.saturating_sub(1),
            Direction::Forward => index.saturating_add(1).min(choices.len().saturating_sub(1)),
        };
        self.choose(queue, choices[next])
    }

    pub(super) fn back(&mut self) -> bool {
        let Some(open) = self.active_mut() else {
            return false;
        };
        if !matches!(open.journey, Journey::Remember { .. }) {
            return false;
        }
        open.journey = Journey::Review(ApprovalChoice::Deny);
        true
    }

    pub(super) fn submission(&mut self, queue: &AttentionQueue) -> Option<ApprovalSubmission> {
        let view = self.view(queue)?;
        let (to, approval_id, offer) = (
            view.agent_id.clone(),
            view.approval_id.clone(),
            view.remember.cloned(),
        );
        let open = self.active_mut()?;
        let decision = match open.journey {
            Journey::Review(ApprovalChoice::Remember) => {
                open.journey = Journey::Remember {
                    selected: ApprovalChoice::ThisSession,
                    offer: offer?.id,
                };
                return None;
            }
            Journey::Review(ApprovalChoice::AllowOnce) => ApprovalDecision::AllowOnce,
            Journey::Review(ApprovalChoice::Deny) => ApprovalDecision::Deny,
            Journey::Remember {
                selected: ApprovalChoice::Back,
                ..
            } => {
                open.journey = Journey::Review(ApprovalChoice::Deny);
                return None;
            }
            Journey::Remember {
                selected,
                offer: expected,
            } => {
                let offered = offer?;
                if offered.id != expected {
                    open.journey = Journey::Review(ApprovalChoice::Deny);
                    open.feedback = Some(ApprovalFeedback::PolicyChanged);
                    return None;
                }
                let scope = match selected {
                    ApprovalChoice::ThisSession => PermissionScope::Session,
                    ApprovalChoice::ThisProject => PermissionScope::Project,
                    _ => return None,
                };
                if !offered.scopes.contains(scope) {
                    return None;
                }
                ApprovalDecision::AllowAndRemember {
                    offer: offered.id,
                    scope,
                }
            }
            Journey::Review(_) | Journey::Submitting(_) => return None,
        };
        open.journey = Journey::Submitting(decision);
        open.feedback = None;
        Some(ApprovalSubmission {
            to,
            approval_id,
            decision,
        })
    }

    pub(super) fn refused(&mut self, attention: &AttentionId, feedback: ApprovalFeedback) {
        let open = self
            .primary
            .iter_mut()
            .chain(self.background.iter_mut())
            .find(|open| &open.attention_id == attention);
        if let Some(open) = open {
            if feedback != ApprovalFeedback::Preparing {
                open.journey = Journey::Review(ApprovalChoice::Deny);
            }
            open.feedback = Some(feedback);
        }
    }

    pub(super) fn dismiss(&mut self) -> Option<SurfaceId> {
        self.background.take().map(|open| open.return_focus)
    }

    pub(super) fn resolved(&mut self, attention_id: &AttentionId) -> Option<SurfaceId> {
        if self
            .background
            .as_ref()
            .is_some_and(|open| &open.attention_id == attention_id)
        {
            return self.dismiss();
        }
        if self
            .primary
            .as_ref()
            .is_some_and(|open| &open.attention_id == attention_id)
        {
            let removed = self.primary.take();
            return if self.background.is_none() {
                removed.map(|open| open.return_focus)
            } else {
                None
            };
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
                reason: plexmaton_core::ApprovalReason::PermissionRequired,
                remember: None,
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

        assert!(
            surface.submission(&queue).is_none(),
            "PER-5: awaiting producer confirmation suppresses duplicates"
        );
        assert!(!surface.move_selection(&queue, Direction::Backward));
        surface.dismiss();
        surface.open(
            AttentionId::new("attention-1").expect("fixture"),
            SurfaceId::Inspector,
        );
        assert!(surface.move_selection(&queue, Direction::Backward));
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
    fn remembered_queue() -> AttentionQueue {
        let mut queue = queue();
        queue.update_offer(
            &AttentionId::new("attention-1").expect("id"),
            Some(plexmaton_core::RememberPermissionOffer {
                note: None,
                id: plexmaton_core::PermissionOfferId::new(4),
                label: "native file changes in this workspace".into(),
                scopes: plexmaton_core::PermissionScopes::SessionAndProject,
            }),
        );
        queue
    }

    /// PER-5: opening the lifetime step is not a decision; only a producer-issued scope can leave it.
    #[test]
    fn per_5_remember_is_two_steps_and_submission_disables_duplicate_decisions() {
        use super::{ApprovalChoice, ApprovalStage};
        let queue = remembered_queue();
        let mut surface = ApprovalSurface::default();
        surface.open(
            AttentionId::new("attention-1").expect("id"),
            SurfaceId::Composer,
        );
        assert!(surface.move_selection(&queue, Direction::Backward));
        assert_eq!(
            surface.view(&queue).expect("view").selected,
            ApprovalChoice::Remember
        );
        assert!(surface.submission(&queue).is_none());
        assert_eq!(
            surface.view(&queue).expect("view").stage,
            ApprovalStage::Remember
        );
        assert!(surface.move_selection(&queue, Direction::Forward));
        let decision = surface.submission(&queue).expect("confirmed scope");
        assert_eq!(
            decision.decision,
            ApprovalDecision::AllowAndRemember {
                offer: plexmaton_core::PermissionOfferId::new(4),
                scope: plexmaton_core::PermissionScope::Project
            }
        );
        assert!(surface.submission(&queue).is_none());
        assert!(surface.view(&queue).expect("waiting").choices().is_empty());
    }

    /// PER-4/PER-5: changed offers discard an unconfirmed selection without minting a replacement decision.
    #[test]
    fn per_4_changed_offer_returns_to_review_and_back_never_grants() {
        use super::{ApprovalChoice, ApprovalFeedback, ApprovalStage};
        let mut queue = remembered_queue();
        let id = AttentionId::new("attention-1").expect("id");
        let mut surface = ApprovalSurface::default();
        surface.open(id.clone(), SurfaceId::Composer);
        surface.choose(&queue, ApprovalChoice::Remember);
        assert!(surface.submission(&queue).is_none());
        assert!(surface.back());
        assert_eq!(
            surface.view(&queue).expect("review").selected,
            ApprovalChoice::Deny
        );
        surface.choose(&queue, ApprovalChoice::Remember);
        assert!(surface.submission(&queue).is_none());
        queue.update_offer(
            &id,
            Some(plexmaton_core::RememberPermissionOffer {
                note: None,
                id: plexmaton_core::PermissionOfferId::new(5),
                label: "new scope".into(),
                scopes: plexmaton_core::PermissionScopes::Session,
            }),
        );
        assert!(surface.submission(&queue).is_none());
        let view = surface.view(&queue).expect("updated request");
        assert_eq!(view.stage, ApprovalStage::Review);
        assert_eq!(view.feedback, Some(ApprovalFeedback::PolicyChanged));
        surface.choose(&queue, ApprovalChoice::Remember);
        assert!(surface.submission(&queue).is_none());
        assert!(!surface.choose(&queue, ApprovalChoice::ThisProject));
    }

    /// ATT-1/PER-5: visiting a worker retains the primary card and its unsubmitted choice.
    #[test]
    fn primary_card_survives_background_visit_without_losing_its_choice() {
        use super::ApprovalChoice;
        let mut queue = queue();
        let primary = AttentionId::new("attention-1").expect("id");
        let worker = AttentionId::new("attention-2").expect("id");
        let mut second = queue.get(&primary).expect("fixture").clone();
        second.id = worker.clone();
        queue.request(second);
        let mut surface = ApprovalSurface::default();
        surface.open(primary.clone(), SurfaceId::Composer);
        surface.choose(&queue, ApprovalChoice::AllowOnce);
        surface.open(worker.clone(), SurfaceId::Inspector);
        assert_eq!(surface.view(&queue).expect("worker").attention_id, &worker);
        surface.dismiss();
        let restored = surface.view(&queue).expect("primary retained");
        assert_eq!(restored.attention_id, &primary);
        assert_eq!(restored.selected, ApprovalChoice::AllowOnce);
    }
}
