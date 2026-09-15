//! Retained product input before a User-controlled child has a live runner ticket.

use std::fmt;

use plexmaton_agent::{Input, UndeliveredInput, UndeliveredReason};

use super::user_input::supported_product_input;
use super::*;
use crate::{UserInputTarget, UserInputTicket};

/// One product submission retained before a cold child has a live runner ticket.
#[derive(Clone, Debug)]
pub struct UserTargetInputRequest {
    target: UserInputTarget,
    input: Input,
    selected_skill: Option<String>,
}

impl UserTargetInputRequest {
    #[must_use]
    pub fn new(target: UserInputTarget, input: Input, selected_skill: Option<String>) -> Self {
        Self {
            target,
            input,
            selected_skill,
        }
    }

    #[must_use]
    pub const fn target(&self) -> &UserInputTarget {
        &self.target
    }

    #[must_use]
    pub const fn input(&self) -> &Input {
        &self.input
    }

    #[must_use]
    pub fn selected_skill(&self) -> Option<&str> {
        self.selected_skill.as_deref()
    }
}

pub(crate) struct PendingUserTargetInput {
    pub(super) request: UserTargetInputRequest,
}

/// Exact target-level input returned when activation or later child admission fails.
pub struct UserTargetInputFailure {
    reason: Box<UserInputRefusal>,
    request: Box<UserTargetInputRequest>,
}

impl UserTargetInputFailure {
    #[must_use]
    pub fn reason(&self) -> &UserInputRefusal {
        self.reason.as_ref()
    }

    #[must_use]
    pub fn into_undelivered(self, reason: UndeliveredReason) -> Option<UndeliveredInput> {
        let request = *self.request;
        let text = match request.input {
            Input::Submitted { text } | Input::Steered { text } => text,
            _ => return None,
        };
        Some(UndeliveredInput::with_skill(
            text,
            request.selected_skill,
            reason,
        ))
    }
}

impl fmt::Debug for UserTargetInputFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserTargetInputFailure")
            .field("reason", &self.reason)
            .field("request", &self.request)
            .finish()
    }
}

impl fmt::Display for UserTargetInputFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(formatter)
    }
}

impl std::error::Error for UserTargetInputFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.reason.as_ref())
    }
}

/// Owner activity produced after target activation, journal linking and input settlement.
#[derive(Debug)]
pub struct OwnedUserInputSettlement {
    worker: MailEndpoint,
    outcome: Box<Result<DispatchReport, UserTargetInputFailure>>,
}

impl OwnedUserInputSettlement {
    #[must_use]
    pub const fn worker(&self) -> &MailEndpoint {
        &self.worker
    }

    pub fn into_outcome(self) -> Result<DispatchReport, UserTargetInputFailure> {
        *self.outcome
    }
}

impl OwnedCollaboration {
    pub(super) fn interrupt_user_target_input(&mut self, conversation: &ConversationId) -> bool {
        let Some(pending) = self.pending_user_target_input.as_ref() else {
            return false;
        };
        if &pending.request.target.worker().conversation != conversation {
            return false;
        }
        let request = self
            .pending_user_target_input
            .take()
            .expect("matched target input remains retained")
            .request;
        let worker = request.target.worker().clone();
        self.pending_user_target_settlement = Some(OwnedUserInputSettlement {
            worker,
            outcome: Box::new(Err(user_target_input_failure(
                UserInputRefusal::Interrupted,
                request,
            ))),
        });
        true
    }

    pub(super) fn has_user_target_input_for(&self, conversation: &ConversationId) -> bool {
        self.pending_user_target_input
            .as_ref()
            .is_some_and(|pending| &pending.request.target.worker().conversation == conversation)
    }

    pub(super) fn clear_user_target_input_for(&mut self, conversation: &ConversationId) {
        if self.has_user_target_input_for(conversation) {
            self.pending_user_target_input.take();
        }
    }

    pub(crate) fn user_target_input_precedes_updates(&self) -> bool {
        let Some(pending) = &self.pending_user_target_input else {
            return false;
        };
        !self
            .pending_stop
            .as_ref()
            .is_some_and(|stop| stop.conversation != pending.request.target.worker().conversation)
    }

    /// Explicitly starts a cold User-controlled child without giving it input or waking a turn.
    pub async fn activate_user_target(
        &mut self,
        target: &UserInputTarget,
    ) -> Result<UserInputTicket, UserInputRefusal> {
        if self.shutting_down {
            return Err(UserInputRefusal::ShuttingDown);
        }
        let canonical = self.require_user_target(target).await?;
        if self
            .runners
            .get(&canonical.worker.conversation)
            .is_some_and(|slot| slot.finished || slot.input_unavailable)
        {
            return Err(UserInputRefusal::RequiresReopen);
        }
        let identity = match self.live_runner_identity(&canonical.delegation, &canonical.worker) {
            Some(identity) => identity,
            None => self
                .resume_user_collaboration_target(target.selector())
                .await
                .map_err(UserInputRefusal::Activation)?,
        };
        let records = self
            .writer
            .records()
            .await
            .map_err(UserInputRefusal::Writer)?;
        let handoff = records
            .iter()
            .rev()
            .find(|record| {
                matches!(
                    &record.event,
                    plexmaton_agent::collaboration::CollaborationEvent::HandoffCompleted {
                        delegation,
                        ..
                    } if delegation == &canonical.delegation
                )
            })
            .ok_or(UserInputRefusal::StaleTarget)?;
        let reference = self.writer.item_reference(&handoff.receipt());
        self.link_child_collaboration_item(&canonical.worker.conversation, reference)
            .await
            .map_err(UserInputRefusal::Runner)?;
        Ok(target.issue_ticket(identity))
    }

    /// Retains one focused product submission without awaiting activation or journal progress.
    pub fn begin_user_target_input(
        &mut self,
        request: UserTargetInputRequest,
    ) -> Result<(), UserTargetInputFailure> {
        if self.pending_user_target_input.is_some()
            || self.pending_user_input.is_some()
            || self.pending_user_target_settlement.is_some()
            || self.detached_user_input.is_some()
            || self.pending_stop.is_some()
            || self.pending_handoff.is_some()
        {
            return Err(user_target_input_failure(
                UserInputRefusal::InProgress,
                request,
            ));
        }
        if self.shutting_down {
            return Err(user_target_input_failure(
                UserInputRefusal::ShuttingDown,
                request,
            ));
        }
        if !supported_product_input(&request.input, request.selected_skill.as_deref()) {
            return Err(user_target_input_failure(
                UserInputRefusal::UnsupportedInput,
                request,
            ));
        }
        if self
            .ingress
            .as_ref()
            .and_then(|ingress| ingress.authenticate_user_target(&request.target))
            .is_none()
        {
            return Err(user_target_input_failure(
                UserInputRefusal::StaleTarget,
                request,
            ));
        }
        self.pending_user_target_input = Some(PendingUserTargetInput { request });
        Ok(())
    }

    pub(crate) async fn finish_pending_user_target_input(&mut self) -> OwnedUserInputSettlement {
        let request = self
            .pending_user_target_input
            .as_ref()
            .expect("accepted target input remains retained")
            .request
            .clone();
        let worker = request.target.worker().clone();
        let outcome = self.finish_user_target_input(request.clone()).await;
        self.pending_user_target_input.take();
        OwnedUserInputSettlement {
            worker,
            outcome: Box::new(outcome),
        }
    }

    async fn finish_user_target_input(
        &mut self,
        request: UserTargetInputRequest,
    ) -> Result<DispatchReport, UserTargetInputFailure> {
        if self.pending_user_input.is_none() {
            let ticket = self
                .activate_user_target(&request.target)
                .await
                .map_err(|reason| user_target_input_failure(reason, request.clone()))?;
            let admitted = UserInputRequest::new(
                ticket,
                request.input.clone(),
                request.selected_skill.clone(),
            );
            self.begin_user_input_inner(admitted)
                .await
                .map_err(|failure| {
                    user_target_input_failure(failure.into_reason(), request.clone())
                })?;
        }
        self.finish_pending_user_input()
            .await
            .map_err(|failure| user_target_input_failure(failure.into_reason(), request))
    }

    pub(super) async fn require_user_target(
        &self,
        target: &UserInputTarget,
    ) -> Result<crate::collaboration_ingress::RegisteredTarget, UserInputRefusal> {
        let canonical = self.require_user_target_any_control(target).await?;
        let view = self
            .writer
            .delegation_view(canonical.delegation.clone())
            .await
            .map_err(UserInputRefusal::Writer)?;
        match view.controller {
            plexmaton_agent::collaboration::DelegationController::Main => {
                Err(UserInputRefusal::ControlledByMain)
            }
            plexmaton_agent::collaboration::DelegationController::User => Ok(canonical),
        }
    }
}

fn user_target_input_failure(
    reason: UserInputRefusal,
    request: UserTargetInputRequest,
) -> UserTargetInputFailure {
    UserTargetInputFailure {
        reason: Box::new(reason),
        request: Box::new(request),
    }
}
