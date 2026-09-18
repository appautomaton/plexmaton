//! Process-local authentication for product-owned child input targets.

use super::*;

/// Proof that the product owner issued this exact canonical child target.
///
/// The stable selector carries no authority on its own. This handle also retains the unforgeable
/// owner identity and exact worker endpoint, and a reopened owner issues replacements.
#[derive(Clone)]
pub struct UserInputTarget {
    selector: TargetSelector,
    worker: MailEndpoint,
    authority: Arc<IngressAuthority>,
}

/// Exact live runner incarnation authenticated for post-Handoff User input.
#[derive(Clone)]
pub struct UserInputTicket {
    target: UserInputTarget,
    identity: crate::RunnerIdentity,
}

/// Display-only controller state from the authenticated collaboration owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedChildControl {
    Main,
    HandoffPending,
    User,
}

/// One addressed revisioned controller projection; it carries no execution authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedChildControlSnapshot {
    worker: MailEndpoint,
    revision: u64,
    control: OwnedChildControl,
}

impl RegisteredCollaborationTarget {
    /// Issues a process-local capability for product-owned input routing to this child.
    #[must_use]
    pub fn user_input_target(&self) -> UserInputTarget {
        UserInputTarget {
            selector: self.selector.clone(),
            worker: self.child.worker().clone(),
            authority: Arc::clone(&self.child.authority),
        }
    }
}

impl UserInputTarget {
    /// Canonical child endpoint this capability addresses.
    #[must_use]
    pub const fn worker(&self) -> &MailEndpoint {
        &self.worker
    }

    pub(crate) const fn selector(&self) -> &TargetSelector {
        &self.selector
    }

    pub(crate) fn issue_ticket(&self, identity: crate::RunnerIdentity) -> UserInputTicket {
        UserInputTicket {
            target: self.clone(),
            identity,
        }
    }
}

impl UserInputTicket {
    /// Exact endpoint and process-local generation this ticket may address.
    #[must_use]
    pub const fn identity(&self) -> &crate::RunnerIdentity {
        &self.identity
    }

    /// Canonical target used to request a fresh ticket after explicit activation.
    #[must_use]
    pub const fn target(&self) -> &UserInputTarget {
        &self.target
    }

    #[cfg(test)]
    pub(crate) fn with_generation_for_test(&self, generation: crate::RunnerGeneration) -> Self {
        Self {
            target: self.target.clone(),
            identity: self.identity.with_generation_for_test(generation),
        }
    }
}

impl OwnedChildControlSnapshot {
    #[must_use]
    pub const fn worker(&self) -> &MailEndpoint {
        &self.worker
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn control(&self) -> OwnedChildControl {
        self.control
    }
}

impl std::fmt::Debug for UserInputTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UserInputTarget")
            .field("selector", &self.selector)
            .field("worker", &self.worker)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for UserInputTicket {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UserInputTicket")
            .field("target", &self.target)
            .field("identity", &self.identity)
            .finish()
    }
}

impl CollaborationIngressOwner {
    pub(crate) fn authenticate_user_target(
        &self,
        target: &UserInputTarget,
    ) -> Option<RegisteredTarget> {
        if !Arc::ptr_eq(&target.authority, &self.authority) {
            return None;
        }
        self.targets
            .get(&target.selector)
            .and_then(|registered| (registered.worker == target.worker).then(|| registered.clone()))
    }
}

impl OwnedCollaboration {
    /// Reads one canonical controller snapshot through an owner-issued target.
    pub async fn child_control_snapshot(
        &mut self,
        target: &UserInputTarget,
    ) -> Result<OwnedChildControlSnapshot, crate::UserInputRefusal> {
        let canonical = self.require_user_target_any_control(target).await?;
        let view = self
            .writer
            .delegation_view(canonical.delegation)
            .await
            .map_err(crate::UserInputRefusal::Writer)?;
        Ok(self.project_child_control(
            canonical.worker,
            match view.controller {
                DelegationController::Main => OwnedChildControl::Main,
                DelegationController::User => OwnedChildControl::User,
            },
        ))
    }

    pub(crate) fn pending_handoff_snapshot(
        &mut self,
        attempt: &CollaborationAttempt,
    ) -> Option<OwnedChildControlSnapshot> {
        let delegation = match &attempt.event {
            CollaborationEvent::HandoffCompleted { delegation, .. } => delegation,
            _ => return None,
        };
        let worker = self
            .ingress
            .as_ref()?
            .targets
            .values()
            .find(|target| &target.delegation == delegation)?
            .worker
            .clone();
        Some(self.project_child_control(worker, OwnedChildControl::HandoffPending))
    }

    fn project_child_control(
        &mut self,
        worker: MailEndpoint,
        control: OwnedChildControl,
    ) -> OwnedChildControlSnapshot {
        if let Some(snapshot) = self.control_snapshots.get(&worker.conversation)
            && snapshot.control == control
        {
            return snapshot.clone();
        }
        let revision = self
            .control_snapshots
            .get(&worker.conversation)
            .map(|snapshot| snapshot.revision.saturating_add(1))
            .unwrap_or(0);
        let snapshot = OwnedChildControlSnapshot {
            worker,
            revision,
            control,
        };
        self.control_snapshots
            .insert(snapshot.worker.conversation.clone(), snapshot.clone());
        snapshot
    }
}
