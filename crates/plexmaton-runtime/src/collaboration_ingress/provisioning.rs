//! Canonical child provisioning and explicit recovery.

use super::*;

impl OwnedCollaboration {
    /// Explicitly reconstructs one canonical Main-controlled child without scheduling work.
    pub async fn resume_collaboration_target(
        &mut self,
        selector: &TargetSelector,
    ) -> Result<crate::RunnerIdentity, CollaborationIngressFailure> {
        let target = self
            .ingress
            .as_ref()
            .ok_or(CollaborationIngressRefusal::Closed)?
            .target(selector);
        let target = match target {
            Some(target) => target,
            None => {
                self.register_collaboration_targets().await?;
                self.ingress
                    .as_ref()
                    .ok_or(CollaborationIngressRefusal::Closed)?
                    .target(selector)
                    .ok_or(CollaborationIngressRefusal::UnknownTarget)?
            }
        };
        let view = self.current_target(&target, true).await?;
        if let Some(identity) = self.live_runner_identity(&target.delegation, &view.worker) {
            return Ok(identity);
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
        let journal = self
            .child_factory
            .as_ref()
            .expect("preflighted child factory")
            .reserve(view.worker.conversation.clone())?;
        let binding = self.delegated_binding(target.delegation.clone()).await?;
        let registered = self
            .register_collaboration_target(target.delegation)
            .await?;
        let runtime = self
            .child_factory
            .as_ref()
            .expect("preflighted child factory")
            .build(view.worker, journal, binding, registered.child_ingress())
            .await?;
        match self.register(runtime).await {
            Ok(identity) => Ok(identity),
            Err(error) => {
                let (registration, mut runtime) = error.into_parts();
                match runtime.shutdown().await {
                    Ok(_) => Err(CollaborationIngressFailure::Registration(registration)),
                    Err(source) => Err(CollaborationIngressFailure::RegistrationCleanup {
                        registration,
                        source: Box::new(source),
                    }),
                }
            }
        }
    }

    pub(super) async fn finish_delegation(
        &mut self,
        delegation: PendingDelegation,
    ) -> Result<CollaborationIngressOutcome, CollaborationIngressFailure> {
        let receipt = loop {
            match self.admit(delegation.creation.clone()).await {
                Ok(receipt) => break receipt,
                Err(crate::CollaborationWriterError::AdmissionBusy { .. }) => {
                    self.writer.wait_writable().await?;
                }
                Err(error) => return Err(error.into()),
            }
        };
        let selector = TargetSelector::issued_for(&self.writer.item_reference(&receipt));
        let canonical = match &delegation.creation.event {
            CollaborationEvent::DelegationCreated { delegation, .. } => delegation.clone(),
            _ => unreachable!("pending delegation retains its creation event"),
        };
        self.register_collaboration_target(canonical)
            .await
            .map_err(|source| provisioning_failure(selector.clone(), source))?;
        let identity = self
            .resume_collaboration_target(&selector)
            .await
            .map_err(|source| provisioning_failure(selector.clone(), source))?;
        self.wake(WakeHint::new(
            identity,
            delegation.wake_item,
            delegation.wake_turn,
        ))
        .map_err(CollaborationIngressFailure::from)
        .map_err(|source| provisioning_failure(selector.clone(), source))?;
        Ok(CollaborationIngressOutcome::Delegated { target: selector })
    }
}

fn provisioning_failure(
    target: TargetSelector,
    source: CollaborationIngressFailure,
) -> CollaborationIngressFailure {
    CollaborationIngressFailure::Provisioning {
        target,
        source: Box::new(source),
    }
}
