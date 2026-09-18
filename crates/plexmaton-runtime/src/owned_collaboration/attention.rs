//! Process-local routing for producer-owned background approval decisions.

use plexmaton_agent::{ApprovalDecisionRefusal, Input, UnresolvedApprovalDecision};
use plexmaton_core::{ApprovalDecision, ApprovalId};

use super::*;
use crate::{RunnerGeneration, UserInputTarget};

impl OwnedCollaboration {
    /// Gives one approval decision to an exact live child incarnation without changing control.
    ///
    /// The target authenticates this owner and endpoint; the generation prevents a restored or
    /// replaced runner from inheriting process-local decision authority. The producer still owns
    /// APV-4 validation and returns stale or wrong request identities in its DispatchReport.
    pub fn begin_attention_decision(
        &mut self,
        target: &UserInputTarget,
        generation: RunnerGeneration,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<(), UnresolvedApprovalDecision> {
        let refusal = |reason| UnresolvedApprovalDecision {
            approval_id: approval_id.clone(),
            decision,
            reason,
            current_offer: None,
        };
        if self.pending_user_input.is_some()
            || self.pending_user_target_input.is_some()
            || self.pending_user_target_settlement.is_some()
            || self.detached_user_input.is_some()
        {
            return Err(refusal(ApprovalDecisionRefusal::Preparing));
        }
        if self.shutting_down || self.pending_stop.is_some() || self.pending_handoff.is_some() {
            return Err(refusal(ApprovalDecisionRefusal::NotPending));
        }
        let Some(canonical) = self
            .ingress
            .as_ref()
            .and_then(|ingress| ingress.authenticate_user_target(target))
        else {
            return Err(refusal(ApprovalDecisionRefusal::NotPending));
        };
        let conversation = canonical.worker.conversation.clone();
        let Some(slot) = self.runners.get_mut(&conversation) else {
            return Err(refusal(ApprovalDecisionRefusal::NotPending));
        };
        if slot.finished
            || slot.delegation != canonical.delegation
            || slot.runner.identity().endpoint() != &canonical.worker
            || slot.runner.identity().generation() != generation
        {
            return Err(refusal(ApprovalDecisionRefusal::NotPending));
        }
        let identity = slot.runner.identity().clone();
        let input = Input::ApprovalDecided {
            approval_id,
            decision,
        };
        let request = UserInputRequest::new(target.issue_ticket(identity), input.clone(), None);
        let Input::ApprovalDecided {
            approval_id: routed_id,
            decision: routed_decision,
        } = input
        else {
            unreachable!("Attention decisions retain approval input")
        };
        if let Err(error) = slot
            .runner
            .begin_attention_decision(routed_id, routed_decision)
        {
            let reason = if matches!(error, OwnedRunnerError::UserInputBusy) {
                ApprovalDecisionRefusal::Preparing
            } else {
                ApprovalDecisionRefusal::NotPending
            };
            let (_, input, _) = request.into_parts();
            let Input::ApprovalDecided {
                approval_id,
                decision,
            } = input
            else {
                unreachable!("Attention decisions retain approval input")
            };
            return Err(UnresolvedApprovalDecision {
                approval_id,
                decision,
                reason,
                current_offer: None,
            });
        }
        self.pending_user_input = Some(PendingUserInput {
            conversation,
            request,
        });
        Ok(())
    }
}
