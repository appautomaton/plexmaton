//! Turning one authenticated tool request into the work its settlement will perform.
//!
//! Split from the lane that settles it because the two answer different questions. This half reads
//! canonical state and decides what should happen; it holds no mutable owner and starts nothing, so
//! whatever it needs built is named in the value it returns. The lane is what then performs it.

use super::*;

impl OwnedCollaboration {
    pub(super) async fn prepare_pending_ingress(
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
        // Mail does not revive its recipient. A letter is a durable atom that waits in the inbox,
        // which is what makes delegation asynchronous; an assignment is what asks a child to act.
        Ok(mail_attempt(
            sender,
            view.worker,
            intent.summary.clone(),
            artifacts,
            wake,
            None,
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
        // An assignment asks a child to act, so a sleeping one is rebuilt for it and a rebuild
        // that fails fails the call — reporting success for work nothing will perform is the one
        // outcome worse than refusing. An owner holding no child factory cannot host a child at
        // all; it keeps the bare admission, which is all such an owner could ever have meant.
        let revive =
            (wake.is_none() && self.child_factory.is_some()).then(|| intent.target.clone());
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
            revive,
        })
    }
}

fn mail_attempt(
    from: MailEndpoint,
    to: MailEndpoint,
    summary: plexmaton_agent::collaboration::CollaborationText,
    artifacts: Vec<ArtifactReference>,
    wake: Option<WakeHint>,
    revive: Option<TargetSelector>,
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
        revive,
    }
}

pub(super) fn fresh_item_id() -> CollaborationItemId {
    CollaborationItemId::new(format!("collaboration-item-{}", uuid::Uuid::now_v7()))
        .unwrap_or_else(|_| unreachable!("formatted collaboration item identity is valid"))
}

pub(super) fn fresh_delegation_id() -> DelegationId {
    DelegationId::new(format!("delegation-{}", uuid::Uuid::now_v7()))
        .unwrap_or_else(|_| unreachable!("formatted delegation identity is valid"))
}

pub(super) fn fresh_conversation_id() -> ConversationId {
    ConversationId::new(format!("conversation-{}", uuid::Uuid::now_v7()))
        .unwrap_or_else(|_| unreachable!("formatted Conversation identity is valid"))
}

pub(super) fn fresh_agent_id() -> AgentId {
    AgentId::new(format!("agent-{}", uuid::Uuid::now_v7()))
        .unwrap_or_else(|_| unreachable!("formatted Agent identity is valid"))
}

pub(super) fn fresh_turn_id() -> TurnId {
    TurnId::new(format!("turn-{}", uuid::Uuid::now_v7()))
        .unwrap_or_else(|_| unreachable!("formatted turn identity is valid"))
}
