//! Authenticated product-owned input after durable Handoff.

use std::fmt;

use plexmaton_agent::{Input, UndeliveredInput, UndeliveredReason};
use thiserror::Error;

use super::*;
use crate::{CollaborationIngressFailure, UserInputTarget, UserInputTicket};

/// One product input bound to an owner-issued canonical child target.
#[derive(Clone, Debug)]
pub struct UserInputRequest {
    ticket: UserInputTicket,
    input: Input,
    selected_skill: Option<String>,
}

pub(super) struct PendingUserInput {
    pub(super) conversation: ConversationId,
    pub(super) request: UserInputRequest,
}

impl UserInputRequest {
    /// Retains the exact target, input and optional skill until the child owner accepts them.
    #[must_use]
    pub fn new(ticket: UserInputTicket, input: Input, selected_skill: Option<String>) -> Self {
        Self {
            ticket,
            input,
            selected_skill,
        }
    }

    /// Exact process-authenticated child runner ticket this input names.
    #[must_use]
    pub const fn ticket(&self) -> &UserInputTicket {
        &self.ticket
    }

    /// Exact producer-safe input retained for this child.
    #[must_use]
    pub const fn input(&self) -> &Input {
        &self.input
    }

    /// Optional explicit skill selection retained beside the input.
    #[must_use]
    pub fn selected_skill(&self) -> Option<&str> {
        self.selected_skill.as_deref()
    }

    /// Returns every caller-owned part without exposing the target's authority internals.
    #[must_use]
    pub fn into_parts(self) -> (UserInputTicket, Input, Option<String>) {
        (self.ticket, self.input, self.selected_skill)
    }
}

/// Why an exact child input did not transfer to the owned runtime.
#[derive(Debug, Error)]
pub enum UserInputRefusal {
    #[error("scheduling owner is shutting down")]
    ShuttingDown,
    #[error("another accepted child input must settle first")]
    InProgress,
    #[error("child input was interrupted before activation")]
    Interrupted,
    #[error("the input target does not belong to this collaboration owner")]
    StaleTarget,
    #[error("the input ticket does not name the current child runner generation")]
    StaleTicket,
    #[error("the child runner must be reopened before it can accept more User input")]
    RequiresReopen,
    #[error("Main controls direct input for this delegated Conversation")]
    ControlledByMain,
    #[error("the input variant is reserved for its producer")]
    UnsupportedInput,
    #[error("collaboration authority could not validate user control: {0}")]
    Writer(#[source] CollaborationWriterError),
    #[error("the child runtime could not be activated: {0}")]
    Activation(#[source] CollaborationIngressFailure),
    #[error("the child runner refused the input: {0}")]
    Runner(#[source] OwnedRunnerError),
}

/// Typed refusal that returns the exact child input to its caller.
pub struct UserInputFailure {
    reason: Box<UserInputRefusal>,
    request: Box<UserInputRequest>,
}

impl UserInputFailure {
    /// Stable reason the request did not complete through its owned runtime.
    #[must_use]
    pub fn reason(&self) -> &UserInputRefusal {
        self.reason.as_ref()
    }

    /// Returns the exact target, input and skill for retry or visible restoration.
    #[must_use]
    pub fn into_request(self) -> UserInputRequest {
        *self.request
    }

    pub(super) fn into_reason(self) -> UserInputRefusal {
        *self.reason
    }

    /// Converts retained message text and skill into the caller's chosen return disposition.
    #[must_use]
    pub fn into_undelivered(self, reason: UndeliveredReason) -> Option<UndeliveredInput> {
        let (_, input, selected_skill) = self.into_request().into_parts();
        let text = match input {
            Input::Submitted { text } | Input::Steered { text } => text,
            _ => return None,
        };
        Some(UndeliveredInput::with_skill(text, selected_skill, reason))
    }
}

impl fmt::Debug for UserInputFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserInputFailure")
            .field("reason", &self.reason)
            .field("request", &self.request)
            .finish()
    }
}

impl fmt::Display for UserInputFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(formatter)
    }
}

impl std::error::Error for UserInputFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.reason.as_ref())
    }
}

impl OwnedCollaboration {
    #[cfg(test)]
    pub(crate) fn fail_next_user_input_for_test(&mut self, conversation: &ConversationId) {
        self.runners
            .get_mut(conversation)
            .expect("test runner exists")
            .runner
            .fail_next_user_input_for_test();
    }

    #[cfg(test)]
    pub(crate) fn notify_next_user_input_admission_for_test(
        &mut self,
        conversation: &ConversationId,
    ) -> std::sync::Arc<tokio::sync::Notify> {
        self.runners
            .get_mut(conversation)
            .expect("test runner exists")
            .runner
            .notify_next_user_input_admission_for_test()
    }

    #[cfg(test)]
    pub(crate) async fn hold_user_input_for_test(
        &self,
        conversation: &ConversationId,
    ) -> Result<std::sync::Arc<tokio::sync::Notify>, OwnedRunnerError> {
        let (_entered, release) = self
            .runners
            .get(conversation)
            .ok_or(OwnedRunnerError::Closed)?
            .runner
            .hold_user_input_for_test()
            .await?;
        Ok(release)
    }

    /// Transfers input to the exact live child lane and retains settlement across cancellation.
    pub async fn begin_user_input(
        &mut self,
        request: UserInputRequest,
    ) -> Result<(), UserInputFailure> {
        if self.pending_user_target_input.is_some()
            || self.pending_user_target_settlement.is_some()
            || self.detached_user_input.is_some()
        {
            return Err(user_input_failure(UserInputRefusal::InProgress, request));
        }
        self.begin_user_input_inner(request).await
    }

    pub(super) async fn begin_user_input_inner(
        &mut self,
        request: UserInputRequest,
    ) -> Result<(), UserInputFailure> {
        if self.pending_user_input.is_some()
            || self.pending_stop.is_some()
            || self.pending_handoff.is_some()
        {
            return Err(user_input_failure(UserInputRefusal::InProgress, request));
        }
        if self.shutting_down {
            return Err(user_input_failure(UserInputRefusal::ShuttingDown, request));
        }
        if !supported_product_input(&request.input, request.selected_skill.as_deref()) {
            return Err(user_input_failure(
                UserInputRefusal::UnsupportedInput,
                request,
            ));
        }
        let canonical = match self.require_user_target(request.ticket.target()).await {
            Ok(canonical) => canonical,
            Err(reason) => return Err(user_input_failure(reason, request)),
        };
        let conversation = canonical.worker.conversation.clone();
        let Some(slot) = self.runners.get_mut(&conversation) else {
            return Err(user_input_failure(UserInputRefusal::StaleTicket, request));
        };
        if slot.delegation != canonical.delegation
            || slot.runner.identity().endpoint() != &canonical.worker
        {
            return Err(user_input_failure(UserInputRefusal::StaleTarget, request));
        }
        if slot.finished || slot.runner.identity() != request.ticket.identity() {
            return Err(user_input_failure(UserInputRefusal::StaleTicket, request));
        }
        if slot.input_unavailable {
            return Err(user_input_failure(
                UserInputRefusal::RequiresReopen,
                request,
            ));
        }
        let retained = request.clone();
        if let Err(error) = slot
            .runner
            .begin_user_input(request.input, request.selected_skill)
        {
            return Err(user_input_failure(
                UserInputRefusal::Runner(error),
                retained,
            ));
        }
        self.pending_user_input = Some(PendingUserInput {
            conversation,
            request: retained,
        });
        Ok(())
    }

    /// Waits for one accepted child input; dropping this wait leaves it owner-retained.
    pub async fn submit_user_input(
        &mut self,
        request: UserInputRequest,
    ) -> Result<DispatchReport, UserInputFailure> {
        self.begin_user_input(request).await?;
        self.finish_pending_user_input().await
    }

    pub(super) async fn finish_pending_user_input(
        &mut self,
    ) -> Result<DispatchReport, UserInputFailure> {
        let conversation = self
            .pending_user_input
            .as_ref()
            .expect("accepted user input remains retained")
            .conversation
            .clone();
        let result = match self.runners.get_mut(&conversation) {
            Some(slot) => slot.runner.finish_user_input().await,
            None => Err(OwnedRunnerError::Closed),
        };
        let requires_reopen = match &result {
            Ok(report) => report.persistence_failure.is_some(),
            Err(OwnedRunnerError::Runtime(RuntimeError::JournalRequiresReopen)) => true,
            Err(_) => false,
        };
        if requires_reopen && let Some(slot) = self.runners.get_mut(&conversation) {
            slot.input_unavailable = true;
        }
        let pending = self
            .pending_user_input
            .take()
            .expect("settled input retains its request");
        result.map_err(|error| user_input_failure(UserInputRefusal::Runner(error), pending.request))
    }

    pub(crate) async fn require_user_target_any_control(
        &self,
        target: &UserInputTarget,
    ) -> Result<crate::collaboration_ingress::RegisteredTarget, UserInputRefusal> {
        let canonical = self
            .ingress
            .as_ref()
            .and_then(|ingress| ingress.authenticate_user_target(target))
            .ok_or(UserInputRefusal::StaleTarget)?;
        let view = self
            .writer
            .delegation_view(canonical.delegation.clone())
            .await
            .map_err(UserInputRefusal::Writer)?;
        if view.worker != canonical.worker {
            return Err(UserInputRefusal::StaleTarget);
        }
        Ok(canonical)
    }
}

pub(super) fn supported_product_input(input: &Input, selected_skill: Option<&str>) -> bool {
    match input {
        Input::Submitted { .. } | Input::Steered { .. } => true,
        Input::ApprovalDecided { .. } => selected_skill.is_none(),
        Input::Streamed { .. }
        | Input::SkillSubmitted { .. }
        | Input::SkillSteered { .. }
        | Input::Failed { .. }
        | Input::ToolAdmissionResolved(_)
        | Input::PermissionsChanged
        | Input::PermissionPrepared(_)
        | Input::ToolFinished { .. }
        | Input::Interrupted
        | Input::ShuttingDown => false,
    }
}

fn user_input_failure(reason: UserInputRefusal, request: UserInputRequest) -> UserInputFailure {
    UserInputFailure {
        reason: Box::new(reason),
        request: Box::new(request),
    }
}
