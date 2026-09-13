//! Canonical selector resolution and cancellation-safe mutation settlement.

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
        Some(self.finish_pending_ingress().await)
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
                return Some(OwnedCollaborationActivity::Ingress(
                    self.finish_pending_ingress().await,
                ));
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

    async fn finish_pending_ingress(&mut self) -> CollaborationIngressSettlement {
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
                            return self.complete_pending_ingress(Err(
                                CollaborationIngressFailure::from(error),
                            ));
                        }
                    },
                    Err(failure) => return self.complete_pending_ingress(Err(failure)),
                }
            }
            let prepared = self
                .pending_ingress
                .as_ref()
                .and_then(|pending| pending.prepared.as_ref())
                .expect("prepared ingress")
                .clone();
            let result = match prepared {
                PreparedIngress::Delegation(delegation) => self.finish_delegation(delegation).await,
                PreparedIngress::Admission {
                    attempt,
                    outcome,
                    wake,
                } => match self.admit(attempt).await {
                    Ok(_) => {
                        if let Some(wake) = wake {
                            let _advisory = self.wake(wake);
                        }
                        Ok(outcome)
                    }
                    Err(crate::CollaborationWriterError::AdmissionBusy { .. }) => {
                        match self.writer.wait_writable().await {
                            Ok(()) => continue,
                            Err(error) => Err(CollaborationIngressFailure::from(error)),
                        }
                    }
                    Err(error) => Err(CollaborationIngressFailure::from(error)),
                },
                PreparedIngress::Handoff(attempt) => self
                    .handoff(attempt)
                    .await
                    .map(|_| CollaborationIngressOutcome::HandoffCompleted)
                    .map_err(CollaborationIngressFailure::from),
            };
            return self.complete_pending_ingress(result);
        }
    }

    async fn prepare_pending_ingress(
        &self,
    ) -> Result<PreparedIngress, CollaborationIngressFailure> {
        let pending = self.pending_ingress.as_ref().expect("pending ingress");
        let ingress = self
            .ingress
            .as_ref()
            .ok_or(CollaborationIngressRefusal::Closed)?;
        match (&pending.command.caller, &pending.command.request) {
            (caller, CollaborationToolRequest::Delegate(_)) => {
                if !ingress.authenticate_main(caller) {
                    return Err(CollaborationIngressRefusal::CapabilityMismatch.into());
                }
                let factory = self
                    .child_factory
                    .as_ref()
                    .ok_or(CollaborationIngressRefusal::ProviderUnsupported)?;
                match factory.preflight() {
                    Ok(()) => {}
                    Err(crate::DelegatedChildFactoryError::ProviderUnsupported) => {
                        return Err(CollaborationIngressRefusal::ProviderUnsupported.into());
                    }
                    Err(error) => return Err(error.into()),
                }
                if !self.has_runner_capacity() {
                    return Err(CollaborationIngressRefusal::Busy.into());
                }
                let CollaborationToolRequest::Delegate(intent) = &pending.command.request else {
                    unreachable!("matched delegation intent")
                };
                let delegation = fresh_delegation_id();
                let worker = MailEndpoint {
                    conversation: fresh_conversation_id(),
                    agent: fresh_agent_id(),
                };
                Ok(PreparedIngress::Delegation(PendingDelegation {
                    creation: CollaborationAttempt {
                        id: fresh_item_id(),
                        event: CollaborationEvent::DelegationCreated {
                            delegation,
                            delegator: ingress
                                .main
                                .clone()
                                .ok_or(CollaborationIngressRefusal::Closed)?,
                            worker: worker.clone(),
                            task: intent.task.clone(),
                        },
                    },
                    wake_item: fresh_item_id(),
                    wake_turn: fresh_turn_id(),
                }))
            }
            (caller, CollaborationToolRequest::MainMail(intent)) => {
                if !ingress.authenticate_main(caller) {
                    return Err(CollaborationIngressRefusal::CapabilityMismatch.into());
                }
                let target = ingress
                    .target(&intent.target)
                    .ok_or(CollaborationIngressRefusal::UnknownTarget)?;
                self.prepare_main_mail(intent, target).await
            }
            (caller, CollaborationToolRequest::ChildMail(intent)) => {
                let control = ingress
                    .authenticate_child(caller)
                    .ok_or(CollaborationIngressRefusal::CapabilityMismatch)?;
                self.prepare_child_mail(intent, control).await
            }
            (caller, CollaborationToolRequest::UpdateTask(intent)) => {
                if !ingress.authenticate_main(caller) {
                    return Err(CollaborationIngressRefusal::CapabilityMismatch.into());
                }
                let target = ingress
                    .target(&intent.target)
                    .ok_or(CollaborationIngressRefusal::UnknownTarget)?;
                self.prepare_task_update(intent, target).await
            }
            (caller, CollaborationToolRequest::Handoff(intent)) => {
                if !ingress.authenticate_main(caller) {
                    return Err(CollaborationIngressRefusal::CapabilityMismatch.into());
                }
                let target = ingress
                    .target(&intent.target)
                    .ok_or(CollaborationIngressRefusal::UnknownTarget)?;
                let view = self.current_target(&target, true).await?;
                Ok(PreparedIngress::Handoff(CollaborationAttempt {
                    id: fresh_item_id(),
                    event: CollaborationEvent::HandoffCompleted {
                        delegation: target.delegation,
                        expected: view.revision,
                        author: view.delegator,
                    },
                }))
            }
        }
    }

    async fn prepare_main_mail(
        &self,
        intent: &MainMailIntent,
        target: RegisteredTarget,
    ) -> Result<PreparedIngress, CollaborationIngressFailure> {
        let sender = self
            .ingress
            .as_ref()
            .expect("ingress")
            .main
            .clone()
            .ok_or(CollaborationIngressRefusal::Closed)?;
        let artifacts = self
            .ingress
            .as_ref()
            .expect("ingress")
            .resolve_artifacts(&sender, &intent.artifacts)?;
        let view = self.current_target(&target, false).await?;
        let wake = self
            .live_runner_identity(&target.delegation, &view.worker)
            .map(|identity| WakeHint::new(identity, fresh_item_id(), fresh_turn_id()));
        Ok(mail_attempt(
            sender,
            view.worker,
            intent.summary.clone(),
            artifacts,
            wake,
        ))
    }

    async fn prepare_child_mail(
        &self,
        intent: &ChildMailIntent,
        control: &DelegatedConversationControl,
    ) -> Result<PreparedIngress, CollaborationIngressFailure> {
        let provenance = control.provenance();
        let view = self
            .delegation_view(provenance.delegation().clone())
            .await?;
        if view.delegator != *provenance.delegator() || view.worker != *provenance.worker() {
            return Err(CollaborationIngressRefusal::StaleTarget.into());
        }
        let artifacts = self
            .ingress
            .as_ref()
            .expect("ingress")
            .resolve_artifacts(&view.worker, &intent.artifacts)?;
        Ok(mail_attempt(
            view.worker,
            view.delegator,
            intent.summary.clone(),
            artifacts,
            None,
        ))
    }

    async fn prepare_task_update(
        &self,
        intent: &UpdateTaskIntent,
        target: RegisteredTarget,
    ) -> Result<PreparedIngress, CollaborationIngressFailure> {
        let view = self.current_target(&target, true).await?;
        let wake = self
            .live_runner_identity(&target.delegation, &view.worker)
            .map(|identity| WakeHint::new(identity, fresh_item_id(), fresh_turn_id()));
        Ok(PreparedIngress::Admission {
            attempt: CollaborationAttempt {
                id: fresh_item_id(),
                event: CollaborationEvent::TaskUpdated {
                    delegation: target.delegation,
                    expected: view.revision,
                    author: view.delegator,
                    task: intent.task.clone(),
                },
            },
            outcome: CollaborationIngressOutcome::TaskUpdated,
            wake,
        })
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
        result: Result<CollaborationIngressOutcome, CollaborationIngressFailure>,
    ) -> CollaborationIngressSettlement {
        let pending = self.pending_ingress.take().expect("pending ingress");
        let reply = result
            .as_ref()
            .map(Clone::clone)
            .map_err(|failure| failure.refusal());
        let reply_delivered = pending.command.reply.send(reply).is_ok();
        CollaborationIngressSettlement {
            call_id: pending.command.call_id,
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
            let settlement = self.finish_pending_ingress().await;
            self.shutdown_settlements
                .push(crate::OwnedShutdownSettlement::Ingress(settlement));
        }
    }
}

fn mail_attempt(
    from: MailEndpoint,
    to: MailEndpoint,
    summary: plexmaton_agent::collaboration::CollaborationText,
    artifacts: Vec<ArtifactReference>,
    wake: Option<WakeHint>,
) -> PreparedIngress {
    PreparedIngress::Admission {
        attempt: CollaborationAttempt {
            id: fresh_item_id(),
            event: CollaborationEvent::MailAccepted {
                mail: MailEnvelope {
                    id: MailId::new(format!("mail-{}", uuid::Uuid::now_v7()))
                        .unwrap_or_else(|_| unreachable!("formatted mail identity is valid")),
                    from,
                    to,
                    summary,
                    artifacts,
                },
            },
        },
        outcome: CollaborationIngressOutcome::MailAccepted,
        wake,
    }
}

fn fresh_item_id() -> CollaborationItemId {
    CollaborationItemId::new(format!("collaboration-item-{}", uuid::Uuid::now_v7()))
        .unwrap_or_else(|_| unreachable!("formatted collaboration item identity is valid"))
}

fn fresh_delegation_id() -> DelegationId {
    DelegationId::new(format!("delegation-{}", uuid::Uuid::now_v7()))
        .unwrap_or_else(|_| unreachable!("formatted delegation identity is valid"))
}

fn fresh_conversation_id() -> ConversationId {
    ConversationId::new(format!("conversation-{}", uuid::Uuid::now_v7()))
        .unwrap_or_else(|_| unreachable!("formatted Conversation identity is valid"))
}

fn fresh_agent_id() -> AgentId {
    AgentId::new(format!("agent-{}", uuid::Uuid::now_v7()))
        .unwrap_or_else(|_| unreachable!("formatted Agent identity is valid"))
}

fn fresh_turn_id() -> TurnId {
    TurnId::new(format!("turn-{}", uuid::Uuid::now_v7()))
        .unwrap_or_else(|_| unreachable!("formatted turn identity is valid"))
}
