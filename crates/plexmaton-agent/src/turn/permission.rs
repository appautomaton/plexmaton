//! Approval identity, preparation, refusal and acknowledged execution intent (APV-4, PER-5).

use plexmaton_core::{ApprovalDecision, ApprovalId, ToolCallStatus};

use crate::{
    Agent, ApprovalDecisionRefusal, Effect, PermissionPreparationOutcome,
    PermissionPreparationRequest, PolicyDecision, Reaction, ToolAuthorization,
    UnresolvedApprovalDecision,
    tools::{ApprovalProgress, ApprovalResolution, PendingApproval},
};

use super::Turn;

impl Agent {
    pub(super) fn approval_decided(
        &mut self,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        reaction: &mut Reaction,
    ) {
        let Some(pending) = self
            .pending_approvals()
            .find(|pending| pending.approval_id() == &approval_id)
            .cloned()
        else {
            Self::refuse_approval_decision(reaction, approval_id, decision);
            return;
        };
        if matches!(pending.progress, ApprovalProgress::Preparing(_)) {
            self.refuse_permission(
                approval_id,
                decision,
                ApprovalDecisionRefusal::Preparing,
                reaction,
            );
            return;
        }
        match decision {
            ApprovalDecision::AllowAndRemember { offer, scope } => {
                let candidate = pending.offer.as_ref().filter(|candidate| {
                    candidate.display.id == offer
                        && candidate.display.scopes.contains(scope)
                        && self.policy.offer_is_current(candidate)
                        && self.policy.remember_offer(pending.admitted()).is_some()
                });
                let Some(candidate) = candidate.cloned() else {
                    self.refuse_permission(
                        approval_id,
                        decision,
                        ApprovalDecisionRefusal::PolicyChanged,
                        reaction,
                    );
                    return;
                };
                if let Some(current) = self.pending_approval_mut(&approval_id) {
                    current.progress = ApprovalProgress::Preparing(decision);
                }
                reaction.effects.push(Effect::PreparePermission(
                    PermissionPreparationRequest::new(
                        approval_id,
                        pending.admitted().clone(),
                        candidate,
                        scope,
                    ),
                ));
            }
            ApprovalDecision::AllowOnce
                if matches!(
                    self.policy.decide(pending.admitted()),
                    PolicyDecision::Forbidden | PolicyDecision::Unavailable
                ) =>
            {
                self.refuse_permission(
                    approval_id,
                    decision,
                    ApprovalDecisionRefusal::PolicyChanged,
                    reaction,
                );
            }
            ApprovalDecision::AllowOnce | ApprovalDecision::Deny => {
                self.finish_approval(approval_id, decision, ToolAuthorization::Once, reaction)
            }
        }
    }

    pub(super) fn permission_prepared(
        &mut self,
        outcome: PermissionPreparationOutcome,
        reaction: &mut Reaction,
    ) {
        let approval_id = match &outcome {
            PermissionPreparationOutcome::Prepared(prepared) => &prepared.approval_id,
            PermissionPreparationOutcome::Refused { approval_id, .. } => approval_id,
        };
        let Some(pending) = self
            .pending_approvals()
            .find(|pending| pending.approval_id() == approval_id)
            .cloned()
        else {
            // Cancellation may finish the call while an owned permission operation is completing.
            return;
        };
        let ApprovalProgress::Preparing(decision) = pending.progress else {
            return;
        };
        match outcome {
            PermissionPreparationOutcome::Prepared(prepared) => {
                if pending.admitted() != &prepared.admitted
                    || pending.offer.as_ref() != Some(&prepared.offer)
                    || decision
                        != (ApprovalDecision::AllowAndRemember {
                            offer: prepared.offer.display.id,
                            scope: prepared.scope,
                        })
                    || self.policy.decide(pending.admitted()) != PolicyDecision::Allow
                {
                    self.refuse_permission(
                        prepared.approval_id,
                        decision,
                        ApprovalDecisionRefusal::PolicyChanged,
                        reaction,
                    );
                    return;
                }
                self.finish_approval(
                    prepared.approval_id,
                    decision,
                    ToolAuthorization::Remembered {
                        grant: prepared.grant,
                        scope: prepared.scope,
                    },
                    reaction,
                );
                self.release_allowed_approvals(reaction);
            }
            PermissionPreparationOutcome::Refused {
                approval_id,
                reason,
            } => self.refuse_permission(
                approval_id,
                decision,
                ApprovalDecisionRefusal::Permission(reason),
                reaction,
            ),
        }
    }

    pub(super) fn release_allowed_approvals(&mut self, reaction: &mut Reaction) {
        let allowed: Vec<_> = self
            .pending_approvals()
            .filter(|pending| {
                pending.progress == ApprovalProgress::Waiting
                    && self.policy.decide(pending.admitted()) == PolicyDecision::Allow
            })
            .map(|pending| pending.approval_id().clone())
            .collect();
        for id in allowed {
            self.finish_approval(
                id,
                ApprovalDecision::AllowOnce,
                ToolAuthorization::Policy,
                reaction,
            );
        }
    }

    pub(super) fn audit_permission(
        &mut self,
        call: &crate::AdmittedToolCall,
        user: Option<crate::PermissionUserDecision>,
        reaction: &mut Reaction,
    ) {
        self.record.commit(
            crate::JournalEntryPayload::ToolPermissionDecided {
                agent_id: self.record.agent_id().clone(),
                call_id: call.requested().call_id.clone(),
                audit: Box::new(self.policy.audit(call, user)),
            },
            reaction,
        );
    }

    fn finish_approval(
        &mut self,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        authorization: ToolAuthorization,
        reaction: &mut Reaction,
    ) {
        let pending = self
            .pending_approvals()
            .find(|pending| pending.approval_id() == &approval_id)
            .cloned();
        if let Some(pending) = pending {
            let user = (!matches!(authorization, ToolAuthorization::Policy)).then(|| {
                crate::PermissionUserDecision {
                    approval_id: approval_id.clone(),
                    decision,
                    reason: self.policy.approval_reason(pending.admitted()),
                    remembered: match &authorization {
                        ToolAuthorization::Remembered { grant, .. } => Some(grant.clone()),
                        _ => None,
                    },
                }
            });
            self.audit_permission(pending.admitted(), user, reaction);
        }
        let resolution = match &mut self.turn {
            Turn::Working { batch, .. } => batch.resolve_approval(&approval_id, decision),
            Turn::Idle | Turn::Streaming { .. } => None,
        };
        match resolution {
            Some(ApprovalResolution::Run {
                attention_id,
                admitted,
            }) => {
                self.resolve_attention(attention_id, reaction);
                self.emit_tool_status(
                    admitted.requested().call_id.clone(),
                    ToolCallStatus::Running,
                    reaction,
                );
                reaction.effects.push(Effect::RunTool {
                    call: admitted,
                    authorization,
                });
            }
            Some(ApprovalResolution::Denied {
                attention_id,
                call_id,
            }) => {
                self.resolve_attention(attention_id, reaction);
                self.emit_tool_status(call_id, ToolCallStatus::Denied, reaction);
                self.continue_if_batch_complete(reaction);
            }
            None => Self::refuse_approval_decision(reaction, approval_id, decision),
        }
    }

    fn refuse_permission(
        &mut self,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        reason: ApprovalDecisionRefusal,
        reaction: &mut Reaction,
    ) {
        let offer = self
            .pending_approvals()
            .find(|pending| pending.approval_id() == &approval_id)
            .and_then(|pending| self.policy.remember_offer(pending.admitted()));
        let current_offer = offer.as_ref().map(|offer| offer.display.clone());
        if reason != ApprovalDecisionRefusal::Preparing
            && let Some(pending) = self.pending_approval_mut(&approval_id)
        {
            pending.progress = ApprovalProgress::Waiting;
            pending.offer = offer;
        }
        reaction
            .unresolved_approvals
            .push(UnresolvedApprovalDecision {
                approval_id,
                decision,
                reason,
                current_offer,
            });
    }

    fn pending_approval_mut(&mut self, approval_id: &ApprovalId) -> Option<&mut PendingApproval> {
        match &mut self.turn {
            Turn::Working { batch, .. } => batch.pending_approval_mut(approval_id),
            Turn::Idle | Turn::Streaming { .. } => None,
        }
    }
}
