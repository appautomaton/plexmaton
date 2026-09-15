//! Canonical child provisioning and explicit recovery.

use super::*;

#[derive(Clone, Copy)]
enum ActivationControl {
    Main,
    User,
}

impl OwnedCollaboration {
    /// Explicitly reconstructs one canonical Main-controlled child without scheduling work.
    pub async fn resume_collaboration_target(
        &mut self,
        selector: &TargetSelector,
    ) -> Result<crate::RunnerIdentity, CollaborationIngressFailure> {
        self.resume_collaboration_target_with_control(selector, ActivationControl::Main)
            .await
    }

    pub(crate) async fn resume_user_collaboration_target(
        &mut self,
        selector: &TargetSelector,
    ) -> Result<crate::RunnerIdentity, CollaborationIngressFailure> {
        self.resume_collaboration_target_with_control(selector, ActivationControl::User)
            .await
    }

    async fn resume_collaboration_target_with_control(
        &mut self,
        selector: &TargetSelector,
        control: ActivationControl,
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
        let view = self
            .current_target(&target, matches!(control, ActivationControl::Main))
            .await?;
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
        let factory = self
            .child_factory
            .as_ref()
            .expect("preflighted child factory");
        let journal = match control {
            ActivationControl::Main => factory.reserve(view.worker.conversation.clone())?,
            ActivationControl::User => factory.resume(view.worker.conversation.clone())?,
        };
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
    ) -> Result<CollaborationIngressResult, CollaborationIngressFailure> {
        let receipt = loop {
            match self.admit(delegation.creation.clone()).await {
                Ok(receipt) => break receipt,
                Err(crate::CollaborationWriterError::AdmissionBusy { .. }) => {
                    self.writer.wait_writable().await?;
                }
                Err(error) => return Err(error.into()),
            }
        };
        let reference = self.writer.item_reference(&receipt);
        let selector = TargetSelector::issued_for(&reference);
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
        Ok(CollaborationIngressResult::new(
            CollaborationIngressOutcome::Delegated { target: selector },
            reference,
        ))
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
