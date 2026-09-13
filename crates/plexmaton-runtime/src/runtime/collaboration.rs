//! Delegated controller routing and permit-retained collaboration turns.

use std::sync::Arc;

use plexmaton_agent::collaboration::ResolvedTurnAdmission;
use plexmaton_agent::collaboration::{
    CollaborationError, CollaborationItemRef, CollaborationLedger, DelegationController,
    ResolvedContext, TurnBoundary,
};
use plexmaton_agent::{ContextAtomValue, Input, ModelCall, UndeliveredReason};
use plexmaton_core::TurnId;
#[cfg(test)]
use plexmaton_core::{CollaborationId, DelegationId};
use plexmaton_session_store::collaboration::{
    DelegatedConversationControl, DelegatedConversationProvenance, ExecutionReservation,
    ExecutionTicket,
};

use super::transition::AfterCommit;
use super::{LiveRuntime, RuntimeInputControl, ShutdownState};
use crate::DispatchReport;
use crate::RuntimeError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UserControlRefusal {
    Main,
    Unavailable,
}

impl LiveRuntime {
    /// Seals this user-owned root's endpoint to its Main ingress and runtime instance.
    pub fn main_collaboration_identity(&self) -> Option<crate::MainRuntimeIdentity> {
        if !matches!(self.input_control, RuntimeInputControl::User) {
            return None;
        }
        self.tools.main_collaboration_identity(
            plexmaton_agent::collaboration::MailEndpoint {
                agent: self.agent_id.clone(),
                conversation: self.agent.journal().conversation_id().clone(),
            },
            Arc::clone(&self.collaboration_identity),
        )
    }

    pub(crate) fn child_collaboration_identity(
        &self,
    ) -> Option<crate::collaboration_ingress::ChildRuntimeIdentity> {
        self.tools.child_collaboration_identity(
            plexmaton_agent::collaboration::MailEndpoint {
                agent: self.agent_id.clone(),
                conversation: self.agent.journal().conversation_id().clone(),
            },
            Arc::clone(&self.collaboration_identity),
        )
    }

    /// Projects artifact facts through this runtime's sealed role and instance identity.
    pub fn collaboration_artifact_source(
        &self,
    ) -> Result<Option<crate::CollaborationArtifactSource>, plexmaton_agent::JournalError> {
        let selected = self
            .agent
            .journal()
            .artifact_origins_on(self.agent.selected_head())?
            .into_iter()
            .filter(|origin| origin.agent() == &self.agent_id)
            .collect();
        let retained = self.agent.journal().retained_artifact_origins()?;
        Ok(self.tools.collaboration_artifact_source(
            plexmaton_agent::collaboration::MailEndpoint {
                agent: self.agent_id.clone(),
                conversation: self.agent.journal().conversation_id().clone(),
            },
            selected,
            retained,
            Arc::clone(&self.collaboration_identity),
        ))
    }

    /// Seals one selected session snapshot to this exact runtime instance and capability.
    pub fn collaboration_session_source(&self) -> Option<crate::CollaborationSessionSource> {
        self.tools.collaboration_session_source(
            plexmaton_agent::collaboration::MailEndpoint {
                agent: self.agent_id.clone(),
                conversation: self.agent.journal().conversation_id().clone(),
            },
            self.agent.journal().clone(),
            self.agent.selected_head().clone(),
            Arc::clone(&self.collaboration_identity),
        )
    }

    /// Whether the selected driver has an explicit typed collaboration representation.
    pub(crate) fn supports_collaboration(&self) -> bool {
        self.driver.supports_collaboration()
    }

    /// Internal finalization used only after delegated construction has narrowed capabilities.
    #[cfg(test)]
    pub(super) fn attach_delegated_control(
        &mut self,
        collaboration: &CollaborationId,
        delegation: &DelegationId,
        control: DelegatedConversationControl,
    ) -> Result<(), RuntimeError> {
        if !matches!(
            self.input_control,
            RuntimeInputControl::AwaitingDelegatedControl
        ) {
            return Err(RuntimeError::InputControlAlreadyBound);
        }
        if self.agent.is_running() || self.has_active_work() {
            return Err(RuntimeError::DelegatedControlBusy);
        }
        self.require_collaboration_idle()?;
        if control.collaboration() != collaboration
            || control.delegation() != delegation
            || control.worker().agent != self.agent_id
            || &control.worker().conversation != self.agent.journal().conversation_id()
        {
            return Err(RuntimeError::DelegatedControlMismatch);
        }
        control.controller()?;
        self.input_control = RuntimeInputControl::Delegated(Box::new(control));
        Ok(())
    }

    /// Current durable input owner for a delegated runtime, when one is attached.
    pub fn delegation_controller(&self) -> Result<Option<DelegationController>, RuntimeError> {
        match &self.input_control {
            RuntimeInputControl::User => Ok(None),
            RuntimeInputControl::AwaitingDelegatedControl => {
                Err(RuntimeError::DelegatedControlUnavailable)
            }
            RuntimeInputControl::Delegated(control) => {
                control.controller().map(Some).map_err(RuntimeError::from)
            }
        }
    }

    /// Canonical child origin attached before this delegated runtime wrote any record.
    pub fn delegated_provenance(
        &self,
    ) -> Result<Option<&DelegatedConversationProvenance>, RuntimeError> {
        match &self.input_control {
            RuntimeInputControl::User => Ok(None),
            RuntimeInputControl::AwaitingDelegatedControl => {
                Err(RuntimeError::DelegatedControlUnavailable)
            }
            RuntimeInputControl::Delegated(control) => Ok(Some(control.provenance())),
        }
    }

    /// Captures the acknowledged idle session boundary for the collaboration log's ordering point.
    pub fn collaboration_boundary(
        &self,
        turn: TurnId,
    ) -> Result<(TurnBoundary, Option<CollaborationItemRef>), RuntimeError> {
        self.require_collaboration_idle()?;
        let boundary = self.agent.collaboration_boundary(turn)?;
        let previous = self
            .agent
            .journal()
            .previous_collaboration_inclusion(self.agent.selected_head())
            .map_err(|_| CollaborationError::InvalidTurnBoundary)?;
        Ok((boundary, previous))
    }

    /// Rebuilds only the current context's referenced admissions; reads no files and starts no work.
    pub fn restore_collaboration_context(
        &mut self,
        ledger: &CollaborationLedger,
    ) -> Result<(), RuntimeError> {
        let resolved = self
            .collaboration_references()?
            .iter()
            .map(|reference| ledger.resolve_turn(reference))
            .collect::<Result<Vec<_>, _>>()?;
        self.restore_resolved_collaboration_context(resolved)
    }

    pub(crate) fn collaboration_references(
        &self,
    ) -> Result<Vec<CollaborationItemRef>, RuntimeError> {
        self.require_collaboration_idle()?;
        let request = self
            .agent
            .journal()
            .project(self.agent.selected_head())
            .map_err(|_| CollaborationError::InvalidTurnBoundary)?
            .into_request();
        Ok(request
            .atoms
            .iter()
            .filter_map(|atom| match atom.value() {
                ContextAtomValue::Collaboration(value) => Some(value.reference().clone()),
                _ => None,
            })
            .collect())
    }

    pub(crate) fn restore_resolved_collaboration_context(
        &mut self,
        resolved: Vec<Arc<ResolvedTurnAdmission>>,
    ) -> Result<(), RuntimeError> {
        self.require_collaboration_idle()?;
        let mut request = self
            .agent
            .journal()
            .project(self.agent.selected_head())
            .map_err(|_| CollaborationError::InvalidTurnBoundary)?
            .into_request();
        let mut context = ResolvedContext::default();
        for admission in resolved {
            context.insert(admission)?;
        }
        context.resolve(&mut request, self.agent.journal())?;
        self.collaboration_context = context;
        Ok(())
    }

    /// Starts one Main-owned child turn and retains its exact permit through owned work.
    pub(crate) async fn start_delegated_turn(
        &mut self,
        reservation: ExecutionReservation,
        ticket: ExecutionTicket,
        resolved: Arc<ResolvedTurnAdmission>,
    ) -> Result<DispatchReport, RuntimeError> {
        self.start_collaboration_turn_inner(resolved, Some((reservation, ticket)))
            .await
    }

    /// Exercises generic collaboration inclusion without production execution authority.
    #[cfg(test)]
    pub(crate) async fn start_collaboration_turn(
        &mut self,
        resolved: Arc<ResolvedTurnAdmission>,
    ) -> Result<DispatchReport, RuntimeError> {
        self.start_collaboration_turn_inner(resolved, None).await
    }

    async fn start_collaboration_turn_inner(
        &mut self,
        resolved: Arc<ResolvedTurnAdmission>,
        authority: Option<(ExecutionReservation, ExecutionTicket)>,
    ) -> Result<DispatchReport, RuntimeError> {
        self.require_collaboration_idle()?;
        if !self.driver.supports_collaboration() {
            return Err(CollaborationError::UnsupportedContext.into());
        }
        if let Some((_, ticket)) = authority.as_ref() {
            self.attached_delegated_control()
                .ok_or(RuntimeError::DelegatedControlMismatch)?
                .validate_ticket(ticket, &resolved)?;
        }
        let (boundary, previous) =
            self.collaboration_boundary(resolved.admission().boundary.turn.clone())?;
        if boundary != resolved.admission().boundary || previous != resolved.admission().previous {
            return Err(CollaborationError::InvalidTurnBoundary.into());
        }
        // Resolve prior inclusions before writing a new boundary; a missing source must not
        // consume the new admission merely to discover that history cannot be materialized.
        let mut prior = self
            .agent
            .journal()
            .project(self.agent.selected_head())
            .map_err(|_| CollaborationError::InvalidTurnBoundary)?
            .into_request();
        self.collaboration_context
            .resolve(&mut prior, self.agent.journal())?;
        let mut next_context = self.collaboration_context.clone();
        next_context.insert(Arc::clone(&resolved))?;
        let permission_snapshot = self.permissions.snapshot()?;
        self.collaboration_permit = match authority {
            Some((reservation, ticket)) => {
                let permit = reservation.bind(ticket)?;
                self.attached_delegated_control()
                    .ok_or(RuntimeError::DelegatedControlMismatch)?
                    .validate_execution(&permit, &resolved)?;
                Some(permit)
            }
            None => None,
        };
        self.collaboration_context = next_context;
        self.agent.use_permission_snapshot(permission_snapshot);
        let reaction = match self
            .agent
            .start_collaboration_turn(&resolved, self.clock.now())
        {
            Ok(reaction) => reaction,
            Err(error) => {
                self.collaboration_permit = None;
                return Err(error.into());
            }
        };
        if let Err(error) = self.begin_transition(reaction, Vec::new(), AfterCommit::None) {
            if self.journal_failed {
                self.finish_failed_owners().await;
            } else {
                self.release_collaboration_permit_if_quiescent();
            }
            return Err(error);
        }
        if let Err(error) = self.finish_transition().await {
            if self.journal_failed {
                self.finish_failed_owners().await;
            } else {
                self.release_collaboration_permit_if_quiescent();
            }
            return Err(error);
        }
        if self.journal_failed {
            self.finish_failed_owners().await;
            return Err(RuntimeError::JournalRequiresReopen);
        }
        Ok(self.take_report())
    }

    pub(super) fn user_control_refusal(&self) -> Option<UserControlRefusal> {
        match &self.input_control {
            RuntimeInputControl::User => None,
            RuntimeInputControl::AwaitingDelegatedControl => Some(UserControlRefusal::Unavailable),
            RuntimeInputControl::Delegated(control) => match control.controller() {
                Ok(DelegationController::User) => None,
                Ok(DelegationController::Main) => Some(UserControlRefusal::Main),
                Err(_) => Some(UserControlRefusal::Unavailable),
            },
        }
    }

    fn attached_delegated_control(&self) -> Option<&DelegatedConversationControl> {
        match &self.input_control {
            RuntimeInputControl::Delegated(control) => Some(control.as_ref()),
            RuntimeInputControl::User | RuntimeInputControl::AwaitingDelegatedControl => None,
        }
    }

    pub(super) fn require_user_control(&self) -> Result<(), RuntimeError> {
        match self.user_control_refusal() {
            None => Ok(()),
            Some(UserControlRefusal::Main) => Err(RuntimeError::ControlledByMain),
            Some(UserControlRefusal::Unavailable) => Err(RuntimeError::DelegatedControlUnavailable),
        }
    }

    pub(super) fn refuse_direct_input(
        &mut self,
        input: &Input,
        selected_skill: Option<&str>,
    ) -> Option<Result<DispatchReport, RuntimeError>> {
        if matches!(input, Input::Interrupted | Input::PermissionsChanged) {
            return None;
        }
        let refusal = self.user_control_refusal()?;
        let reason = match refusal {
            UserControlRefusal::Main => UndeliveredReason::ControlledByMain,
            UserControlRefusal::Unavailable => UndeliveredReason::ControlUnavailable,
        };
        if let Some(input) = super::rejected_user_input(input, selected_skill, reason) {
            self.report.undelivered.push(input);
            return Some(Ok(self.take_report()));
        }
        Some(Err(match refusal {
            UserControlRefusal::Main => RuntimeError::ControlledByMain,
            UserControlRefusal::Unavailable => RuntimeError::DelegatedControlUnavailable,
        }))
    }

    pub(super) fn release_collaboration_permit_if_quiescent(&mut self) {
        if self.collaboration_permit.is_some()
            && self.pending_commit.is_none()
            && self.after_commit.is_none()
            && self.active.is_none()
            && self.pending_model_start.is_none()
            && self.deferred_model_call.is_none()
            && self.deferred_compaction_failure.is_none()
            && self.pending_inputs.is_empty()
            && self.preparing_input.is_none()
            && self.compaction.is_none()
            && self.tools.is_empty()
            && !self.agent.is_running()
            && self.agent.pending_approvals().next().is_none()
            && self.agent.queued_for_next_step().next().is_none()
            && self.agent.queued_for_next_turn().next().is_none()
        {
            self.collaboration_permit = None;
        }
    }

    pub(super) fn resolve_collaboration_call(
        &self,
        call: &mut ModelCall,
    ) -> Result<(), CollaborationError> {
        if call
            .request
            .atoms
            .iter()
            .any(|atom| matches!(atom.value(), ContextAtomValue::Collaboration(_)))
            && !self.driver.supports_collaboration()
        {
            return Err(CollaborationError::UnsupportedContext);
        }
        self.collaboration_context
            .resolve(&mut call.request, self.agent.journal())
    }

    fn require_collaboration_idle(&self) -> Result<(), RuntimeError> {
        if self.journal.is_none() {
            return Err(CollaborationError::PersistenceRequired.into());
        }
        if self.journal_failed {
            return Err(RuntimeError::JournalRequiresReopen);
        }
        if self.shutdown_state != ShutdownState::Open {
            return Err(RuntimeError::ShuttingDown);
        }
        if self.pending_commit.is_some()
            || self.after_commit.is_some()
            || self.active.is_some()
            || self.pending_model_start.is_some()
            || !self.pending_inputs.is_empty()
            || self.preparing_input.is_some()
            || self.compaction.is_some()
            || !self.tools.is_empty()
            || self.collaboration_permit.is_some()
            || self.agent.is_running()
        {
            return Err(CollaborationError::InvalidTurnBoundary.into());
        }
        Ok(())
    }
}
