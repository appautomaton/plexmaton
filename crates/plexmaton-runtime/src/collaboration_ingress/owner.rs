//! Canonical selector resolution and cancellation-safe mutation settlement.

use super::preparation::{fresh_item_id, fresh_turn_id};
use super::*;

impl OwnedCollaboration {
    /// Opens one unbound Main tool lane whose identity must later come from its owning runtime.
    pub fn open_main_ingress(
        &mut self,
    ) -> Result<MainCollaborationIngress, CollaborationIngressRefusal> {
        if self.ingress.is_some() {
            return Err(CollaborationIngressRefusal::CapabilityMismatch);
        }
        let (owner, ingress) = CollaborationIngressOwner::new();
        self.ingress = Some(owner);
        Ok(ingress)
    }

    /// Binds Main authorship only to the user-owned runtime carrying this exact tool lane.
    pub fn bind_main_runtime(
        &mut self,
        identity: MainRuntimeIdentity,
    ) -> Result<(), CollaborationIngressRefusal> {
        self.ingress
            .as_mut()
            .ok_or(CollaborationIngressRefusal::Closed)?
            .bind_main(identity)
    }

    #[cfg(test)]
    pub(crate) fn bind_main_ingress(
        &mut self,
        main: MailEndpoint,
    ) -> Result<MainCollaborationIngress, CollaborationIngressRefusal> {
        let ingress = self.open_main_ingress()?;
        self.bind_main_runtime(ingress.identify(main, RuntimeCollaborationIdentity::fresh()))?;
        Ok(ingress)
    }

    /// Rebuilds one stable target and fixed-parent child capability from canonical provenance.
    pub async fn register_collaboration_target(
        &mut self,
        delegation: DelegationId,
    ) -> Result<RegisteredCollaborationTarget, CollaborationIngressFailure> {
        let control = self.writer.delegated_control(delegation).await?;
        if control
            .controller()
            .map_err(crate::CollaborationWriterError::Store)?
            == DelegationController::User
        {
            self.handoff_closed.insert(control.delegation().clone());
        }
        self.ingress
            .as_mut()
            .ok_or(CollaborationIngressRefusal::Closed)?
            .register(control)
            .map_err(CollaborationIngressFailure::from)
    }

    /// Rebuilds stable selectors and child capabilities from every canonical creation on resume.
    pub async fn register_collaboration_targets(
        &mut self,
    ) -> Result<Vec<RegisteredCollaborationTarget>, CollaborationIngressFailure> {
        let controls = self.writer.delegated_controls().await?;
        let ingress = self
            .ingress
            .as_mut()
            .ok_or(CollaborationIngressRefusal::Closed)?;
        let main = ingress
            .main
            .clone()
            .ok_or(CollaborationIngressRefusal::Closed)?;
        let mut targets = Vec::new();
        for control in controls {
            if control.provenance().delegator() == &main {
                if control
                    .controller()
                    .map_err(crate::CollaborationWriterError::Store)?
                    == DelegationController::User
                {
                    self.handoff_closed.insert(control.delegation().clone());
                }
                targets.push(ingress.register(control)?);
            }
        }
        Ok(targets)
    }

    /// Issues stable selectors only for facts sealed by the owning collaboration runtime.
    pub fn register_collaboration_artifacts(
        &mut self,
        source: CollaborationArtifactSource,
    ) -> Result<Vec<crate::ArtifactSelector>, CollaborationArtifactRegistrationError> {
        let ingress = self
            .ingress
            .as_mut()
            .ok_or(CollaborationArtifactRegistrationError::Closed)?;
        if !Arc::ptr_eq(&source.authority, &ingress.authority)
            || !ingress.owns_endpoint(&source.endpoint)
            || !ingress
                .runtimes
                .get(&source.endpoint)
                .is_some_and(|runtime| Arc::ptr_eq(runtime, &source.runtime))
        {
            return Err(CollaborationArtifactRegistrationError::EndpointMismatch);
        }
        let mut staged = Vec::with_capacity(source.selected.len());
        for origin in &source.selected {
            if origin.conversation() != &source.endpoint.conversation
                || origin.agent() != &source.endpoint.agent
            {
                return Err(CollaborationArtifactRegistrationError::EndpointMismatch);
            }
            if source
                .retained
                .iter()
                .filter(|candidate| candidate.artifact() == origin.artifact())
                .count()
                != 1
            {
                return Err(CollaborationArtifactRegistrationError::AmbiguousArtifact(
                    origin.artifact().clone(),
                ));
            }
            let selector = crate::ArtifactSelector::issued_for(origin);
            let reference = ArtifactReference {
                conversation: source.endpoint.conversation.clone(),
                artifact: origin.artifact().clone(),
            };
            if ingress
                .artifacts
                .get(&(source.endpoint.clone(), selector.clone()))
                .is_some_and(|existing| existing != &reference)
                || staged.iter().any(
                    |(candidate, existing): &(crate::ArtifactSelector, ArtifactReference)| {
                        candidate == &selector && existing != &reference
                    },
                )
            {
                return Err(CollaborationArtifactRegistrationError::EndpointMismatch);
            }
            staged.push((selector, reference));
        }
        let selectors = staged
            .iter()
            .map(|(selector, _)| selector.clone())
            .collect();
        ingress.artifacts.extend(
            staged
                .into_iter()
                .map(|(selector, reference)| ((source.endpoint.clone(), selector), reference)),
        );
        Ok(selectors)
    }

    /// Processes one accepted tool command; caller cancellation cannot remove it after receipt.
    pub async fn next_ingress(&mut self) -> Option<CollaborationIngressSettlement> {
        if self.pending_ingress.is_none() {
            let pending = self.ingress.as_mut()?.receive().await?;
            self.pending_ingress = Some(pending);
        }
        loop {
            match self.advance_pending_ingress().await {
                IngressProgress::Control(_) => {}
                IngressProgress::Settled(settlement) => return Some(settlement),
            }
        }
    }

    /// Waits for either an authenticated tool command or one child-runner update.
    pub async fn next_activity(&mut self) -> Option<OwnedCollaborationActivity> {
        loop {
            if self.pending_ingress.is_none() {
                self.pending_ingress = self
                    .ingress
                    .as_mut()
                    .and_then(CollaborationIngressOwner::try_receive);
            }
            if self.pending_ingress.is_some() {
                return Some(match self.advance_pending_ingress().await {
                    IngressProgress::Control(snapshot) => {
                        OwnedCollaborationActivity::Control(snapshot)
                    }
                    IngressProgress::Settled(settlement) => {
                        OwnedCollaborationActivity::Ingress(settlement)
                    }
                });
            }
            if let Some(settlement) = self.pending_user_target_settlement.take() {
                return Some(OwnedCollaborationActivity::UserInput(settlement));
            }
            if self.user_target_input_precedes_updates() {
                return Some(OwnedCollaborationActivity::UserInput(
                    self.finish_pending_user_target_input().await,
                ));
            }
            if let Some(settlement) = self.settle_cold_handoff().await {
                return Some(OwnedCollaborationActivity::Handoff(settlement));
            }
            let ingress = self.ingress.as_ref()?;
            if ingress.receiver.is_closed()
                && ingress.receiver.is_empty()
                && !self.has_update_source()
            {
                return None;
            }
            let notify = Arc::clone(&ingress.notify);
            let notified = notify.notified();
            tokio::pin!(notified);
            if self.pending_ingress.is_none() {
                self.pending_ingress = self
                    .ingress
                    .as_mut()
                    .and_then(CollaborationIngressOwner::try_receive);
            }
            if self.pending_ingress.is_some() {
                continue;
            }
            if !self.has_update_source() {
                notified.await;
                continue;
            }
            tokio::select! {
                biased;
                () = &mut notified => continue,
                update = self.next_update() => {
                    if let Some(update) = update {
                        return Some(OwnedCollaborationActivity::Runner(update));
                    }
                }
            }
        }
    }

    async fn advance_pending_ingress(&mut self) -> IngressProgress {
        loop {
            if self
                .pending_ingress
                .as_ref()
                .is_some_and(|pending| pending.prepared.is_none())
            {
                match self.prepare_pending_ingress().await {
                    Ok(prepared) => {
                        self.pending_ingress
                            .as_mut()
                            .expect("pending ingress")
                            .prepared = Some(prepared);
                    }
                    Err(CollaborationIngressFailure::Writer(
                        crate::CollaborationWriterError::Busy,
                    )) => match self.writer.wait_writable().await {
                        Ok(()) => continue,
                        Err(error) => {
                            return IngressProgress::Settled(self.complete_pending_ingress(Err(
                                CollaborationIngressFailure::from(error),
                            )));
                        }
                    },
                    Err(failure) => {
                        return IngressProgress::Settled(
                            self.complete_pending_ingress(Err(failure)),
                        );
                    }
                }
            }
            let prepared = self
                .pending_ingress
                .as_ref()
                .and_then(|pending| pending.prepared.as_ref())
                .expect("prepared ingress")
                .clone();
            if let PreparedIngress::Handoff(attempt) = &prepared
                && !self
                    .pending_ingress
                    .as_ref()
                    .expect("prepared ingress")
                    .control_announced
            {
                self.pending_ingress
                    .as_mut()
                    .expect("prepared ingress")
                    .control_announced = true;
                return IngressProgress::Control(
                    self.pending_handoff_snapshot(attempt)
                        .expect("authenticated Handoff retains its canonical target"),
                );
            }
            let result = match prepared {
                PreparedIngress::Delegation(delegation) => self.finish_delegation(delegation).await,
                PreparedIngress::Admission {
                    attempt,
                    outcome,
                    wake,
                    revive,
                } => {
                    // Reviving before admitting, because a task recorded for a child that could not
                    // be rebuilt is work the log promises and nothing performs. A refusal here is
                    // the tool call failing, which is the whole point: it used to report success.
                    let wake = match (wake, revive) {
                        (Some(wake), _) => Some(wake),
                        (None, Some(selector)) => {
                            match self.resume_collaboration_target(&selector).await {
                                Ok(identity) => {
                                    Some(WakeHint::new(identity, fresh_item_id(), fresh_turn_id()))
                                }
                                Err(failure) => {
                                    return IngressProgress::Settled(
                                        self.complete_pending_ingress(Err(failure)),
                                    );
                                }
                            }
                        }
                        (None, None) => None,
                    };
                    match self.admit(attempt).await {
                        Ok(receipt) => {
                            if let Some(wake) = wake {
                                let _advisory = self.wake(wake);
                            }
                            let reference = self.writer.item_reference(&receipt);
                            Ok(CollaborationIngressResult::new(outcome, reference))
                        }
                        Err(crate::CollaborationWriterError::AdmissionBusy { .. }) => {
                            match self.writer.wait_writable().await {
                                Ok(()) => continue,
                                Err(error) => Err(CollaborationIngressFailure::from(error)),
                            }
                        }
                        Err(error) => Err(CollaborationIngressFailure::from(error)),
                    }
                }
                PreparedIngress::Handoff(attempt) => self
                    .handoff(attempt)
                    .await
                    .map(|report| {
                        let reference = self.writer.item_reference(&report.receipt);
                        CollaborationIngressResult::new(
                            CollaborationIngressOutcome::HandoffCompleted,
                            reference,
                        )
                    })
                    .map_err(CollaborationIngressFailure::from),
            };
            return IngressProgress::Settled(self.complete_pending_ingress(result));
        }
    }

    pub(super) async fn current_target(
        &self,
        target: &RegisteredTarget,
        require_main: bool,
    ) -> Result<plexmaton_agent::collaboration::DelegationView, CollaborationIngressFailure> {
        let view = self.delegation_view(target.delegation.clone()).await?;
        if view.delegator != target.delegator
            || view.worker != target.worker
            || self
                .ingress
                .as_ref()
                .ok_or(CollaborationIngressRefusal::Closed)?
                .main
                .as_ref()
                != Some(&view.delegator)
            || (require_main && view.controller != DelegationController::Main)
        {
            return Err(CollaborationIngressRefusal::StaleTarget.into());
        }
        Ok(view)
    }

    fn complete_pending_ingress(
        &mut self,
        result: Result<CollaborationIngressResult, CollaborationIngressFailure>,
    ) -> CollaborationIngressSettlement {
        let pending = self.pending_ingress.take().expect("pending ingress");
        let caller = match &pending.command.caller {
            IngressCaller::Main(_) => self
                .ingress
                .as_ref()
                .and_then(|ingress| ingress.main.clone()),
            IngressCaller::Child { control, .. } => Some(control.worker().clone()),
        };
        let reply = result
            .as_ref()
            .map(Clone::clone)
            .map_err(|failure| failure.refusal());
        let reply_delivered = pending.command.reply.send(reply).is_ok();
        CollaborationIngressSettlement {
            call_id: pending.command.call_id,
            caller,
            result,
            reply_delivered,
        }
    }

    pub(crate) async fn settle_ingress_for_shutdown(&mut self) {
        let Some(ingress) = self.ingress.as_mut() else {
            return;
        };
        ingress.close();
        loop {
            if self.pending_ingress.is_none() {
                self.pending_ingress = self.ingress.as_mut().and_then(|owner| owner.try_receive());
            }
            if self.pending_ingress.is_none() {
                break;
            }
            match self.advance_pending_ingress().await {
                IngressProgress::Control(_) => {}
                IngressProgress::Settled(settlement) => self
                    .shutdown_settlements
                    .push(crate::OwnedShutdownSettlement::Ingress(settlement)),
            }
        }
    }
}

enum IngressProgress {
    Control(OwnedChildControlSnapshot),
    Settled(CollaborationIngressSettlement),
}
