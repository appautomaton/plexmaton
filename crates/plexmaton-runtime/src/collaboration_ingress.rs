//! Authenticated bounded ingress from native collaboration tools to their sole mutation owner.

use std::{collections::BTreeMap, sync::Arc};

use plexmaton_agent::collaboration::{
    ArtifactReference, CollaborationEvent, DelegationController, MailEndpoint, MailEnvelope,
};
use plexmaton_core::{
    AgentId, ArtifactId, CollaborationItemId, ConversationId, DelegationId, MailId, ToolCallId,
    TurnId,
};
use plexmaton_session_store::collaboration::{CollaborationAttempt, DelegatedConversationControl};
use thiserror::Error;
use tokio::sync::{Notify, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::{
    ChildMailIntent, CollaborationToolRequest, MainMailIntent, OwnedCollaboration,
    OwnedHandoffFailure, OwnedHandoffSettlement, RuntimeError, TargetSelector, UpdateTaskIntent,
    WakeHint,
};

mod result;
pub use result::{CollaborationIngressResult, CollaborationIngressSettlement};
mod user_target;
pub use user_target::{UserInputTarget, UserInputTicket};

const INGRESS_CAPACITY: usize = 8;

struct IngressAuthority;

/// Cloneable Main-only capability; its private owner identity never enters model arguments.
#[derive(Clone)]
pub struct MainCollaborationIngress {
    sender: mpsc::Sender<IngressCommand>,
    authority: Arc<IngressAuthority>,
    notify: Arc<Notify>,
}

/// Cloneable child-only capability, fixed to canonical provenance when the owner issues it.
#[derive(Clone)]
pub struct ChildCollaborationIngress {
    sender: mpsc::Sender<IngressCommand>,
    authority: Arc<IngressAuthority>,
    control: Arc<DelegatedConversationControl>,
    notify: Arc<Notify>,
}

/// Stable target plus the fixed-parent capability for its delegated Conversation.
pub struct RegisteredCollaborationTarget {
    selector: TargetSelector,
    child: ChildCollaborationIngress,
}

impl RegisteredCollaborationTarget {
    /// Reconstructible model-facing selector for this canonical delegation.
    #[must_use]
    pub const fn selector(&self) -> &TargetSelector {
        &self.selector
    }

    /// Capability that may be installed only in this exact child runtime.
    #[must_use]
    pub fn child_ingress(&self) -> ChildCollaborationIngress {
        self.child.clone()
    }

    /// The child this target addresses, so a runner update can be matched back to it.
    #[must_use]
    pub fn worker(&self) -> &MailEndpoint {
        self.child.worker()
    }

    /// The canonical delegation, so a caller can read the task it currently carries.
    #[must_use]
    pub fn delegation(&self) -> &DelegationId {
        self.child.control.delegation()
    }
}

impl ChildCollaborationIngress {
    pub(crate) fn worker(&self) -> &MailEndpoint {
        self.control.worker()
    }

    pub(crate) fn provenance(
        &self,
    ) -> &plexmaton_session_store::collaboration::DelegatedConversationProvenance {
        self.control.provenance()
    }
}

/// Bounded tool-facing result; durable identities stay in the owner settlement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CollaborationIngressOutcome {
    Delegated { target: TargetSelector },
    MailAccepted,
    TaskUpdated,
    HandoffCompleted,
}

/// Stable refusal returned to a tool without exposing storage or authority internals.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CollaborationIngressRefusal {
    #[error("collaboration ingress is busy")]
    Busy,
    #[error("collaboration ingress is closed")]
    Closed,
    #[error("collaboration caller stopped waiting")]
    Cancelled,
    #[error("collaboration caller capability does not match this operation")]
    CapabilityMismatch,
    #[error("collaboration target is unknown")]
    UnknownTarget,
    #[error("collaboration target no longer matches canonical state")]
    StaleTarget,
    #[error("artifact selector is unknown for the authenticated sender")]
    UnknownArtifact,
    #[error("delegation execution is unavailable until a supported child factory is bound")]
    ProviderUnsupported,
    #[error("delegation {target} was created, but child provisioning is pending")]
    ProvisioningPending { target: TargetSelector },
    #[error("collaboration mutation failed inside its owner")]
    MutationFailed,
}

/// Detailed owner-side failure, including exact retained mutation input where applicable.
#[derive(Debug, Error)]
pub enum CollaborationIngressFailure {
    #[error(transparent)]
    Refused(#[from] CollaborationIngressRefusal),
    #[error("collaboration writer failed: {0}")]
    Writer(#[from] crate::CollaborationWriterError),
    #[error("collaboration Handoff failed: {0}")]
    Handoff(#[from] OwnedHandoffFailure),
    #[error("delegated child factory failed: {0}")]
    Factory(#[from] crate::DelegatedChildFactoryError),
    #[error("delegated runner registration failed after child cleanup: {0}")]
    Registration(crate::RunnerRegistrationReason),
    #[error(
        "delegated runner registration failed ({registration}) and child cleanup failed: {source}"
    )]
    RegistrationCleanup {
        registration: crate::RunnerRegistrationReason,
        source: Box<RuntimeError>,
    },
    #[error("delegated child wake failed: {0}")]
    Wake(#[from] crate::WakeFailure),
    #[error("delegation {target} was created, but child provisioning is pending: {source}")]
    Provisioning {
        target: TargetSelector,
        source: Box<CollaborationIngressFailure>,
    },
}

/// Why canonical session facts could not enter the owner-local artifact selector registry.
#[derive(Debug, Error)]
pub enum CollaborationArtifactRegistrationError {
    #[error("collaboration ingress is not bound")]
    Closed,
    #[error("artifact journal does not match an authenticated collaboration endpoint")]
    EndpointMismatch,
    #[error("artifact identity is ambiguous in its owning Conversation: {0}")]
    AmbiguousArtifact(ArtifactId),
    #[error("artifact journal cannot be projected: {0:?}")]
    Journal(plexmaton_agent::JournalError),
}

impl CollaborationIngressFailure {
    fn refusal(&self) -> CollaborationIngressRefusal {
        match self {
            Self::Refused(refusal) => refusal.clone(),
            Self::Factory(crate::DelegatedChildFactoryError::ProviderUnsupported) => {
                CollaborationIngressRefusal::ProviderUnsupported
            }
            Self::Provisioning { target, .. } => CollaborationIngressRefusal::ProvisioningPending {
                target: target.clone(),
            },
            Self::Writer(_)
            | Self::Handoff(_)
            | Self::Factory(_)
            | Self::Registration(_)
            | Self::RegistrationCleanup { .. }
            | Self::Wake(_) => CollaborationIngressRefusal::MutationFailed,
        }
    }
}

/// One root-facing collaboration activity from either the tool lane or a child runner.
#[derive(Debug)]
pub enum OwnedCollaborationActivity {
    Ingress(CollaborationIngressSettlement),
    Runner(crate::OwnedRunnerUpdate),
    /// A direct Handoff settled without a live runner incarnation to tag.
    Handoff(OwnedHandoffSettlement),
}

enum IngressCaller {
    Main(Arc<IngressAuthority>),
    Child {
        authority: Arc<IngressAuthority>,
        control: Arc<DelegatedConversationControl>,
    },
}

struct IngressCommand {
    call_id: ToolCallId,
    caller: IngressCaller,
    request: CollaborationToolRequest,
    reply: oneshot::Sender<Result<CollaborationIngressResult, CollaborationIngressRefusal>>,
}

#[derive(Clone)]
pub(crate) struct RegisteredTarget {
    pub(crate) delegation: DelegationId,
    pub(crate) delegator: MailEndpoint,
    pub(crate) worker: MailEndpoint,
}

pub(crate) struct CollaborationIngressOwner {
    authority: Arc<IngressAuthority>,
    sender: mpsc::Sender<IngressCommand>,
    receiver: mpsc::Receiver<IngressCommand>,
    main: Option<MailEndpoint>,
    targets: BTreeMap<TargetSelector, RegisteredTarget>,
    artifacts: BTreeMap<(MailEndpoint, crate::ArtifactSelector), ArtifactReference>,
    runtimes: BTreeMap<MailEndpoint, Arc<RuntimeCollaborationIdentity>>,
    notify: Arc<Notify>,
}

pub(crate) struct PendingIngress {
    command: IngressCommand,
    prepared: Option<PreparedIngress>,
}

#[derive(Clone)]
enum PreparedIngress {
    Delegation(PendingDelegation),
    Admission {
        attempt: CollaborationAttempt,
        outcome: CollaborationIngressOutcome,
        wake: Option<WakeHint>,
        /// A child this call needs running that has none yet, named as the model named it.
        ///
        /// A delegation outlives the process that created it, but its runner does not, so after a
        /// resume every target is addressable and none is running. Preparation holds no mutable
        /// owner and cannot build one, so it records which child to build and settlement does it.
        revive: Option<TargetSelector>,
    },
    Handoff(CollaborationAttempt),
}

#[derive(Clone)]
struct PendingDelegation {
    creation: CollaborationAttempt,
    wake_item: CollaborationItemId,
    wake_turn: TurnId,
}

impl CollaborationIngressOwner {
    pub(crate) fn new() -> (Self, MainCollaborationIngress) {
        let authority = Arc::new(IngressAuthority);
        let notify = Arc::new(Notify::new());
        let (sender, receiver) = mpsc::channel(INGRESS_CAPACITY);
        let main_ingress = MainCollaborationIngress {
            sender: sender.clone(),
            authority: Arc::clone(&authority),
            notify: Arc::clone(&notify),
        };
        (
            Self {
                authority,
                sender,
                receiver,
                main: None,
                targets: BTreeMap::new(),
                artifacts: BTreeMap::new(),
                runtimes: BTreeMap::new(),
                notify,
            },
            main_ingress,
        )
    }

    pub(crate) fn bind_main(
        &mut self,
        identity: MainRuntimeIdentity,
    ) -> Result<(), CollaborationIngressRefusal> {
        if self.main.is_some() || !Arc::ptr_eq(&identity.authority, &self.authority) {
            return Err(CollaborationIngressRefusal::CapabilityMismatch);
        }
        self.runtimes
            .insert(identity.endpoint.clone(), identity.runtime);
        self.main = Some(identity.endpoint);
        Ok(())
    }

    pub(crate) fn register(
        &mut self,
        control: DelegatedConversationControl,
    ) -> Result<RegisteredCollaborationTarget, CollaborationIngressRefusal> {
        let provenance = control.provenance();
        if self.main.as_ref() != Some(provenance.delegator()) {
            return Err(CollaborationIngressRefusal::StaleTarget);
        }
        let selector = TargetSelector::issued_for(provenance.creation());
        let target = RegisteredTarget {
            delegation: provenance.delegation().clone(),
            delegator: provenance.delegator().clone(),
            worker: provenance.worker().clone(),
        };
        match self.targets.get(&selector) {
            Some(existing)
                if existing.delegation != target.delegation
                    || existing.delegator != target.delegator
                    || existing.worker != target.worker =>
            {
                return Err(CollaborationIngressRefusal::StaleTarget);
            }
            Some(_) => {}
            None => {
                self.targets.insert(selector.clone(), target);
            }
        }
        Ok(RegisteredCollaborationTarget {
            selector,
            child: ChildCollaborationIngress {
                sender: self.sender.clone(),
                authority: Arc::clone(&self.authority),
                control: Arc::new(control),
                notify: Arc::clone(&self.notify),
            },
        })
    }

    pub(crate) fn close(&mut self) {
        self.receiver.close();
    }

    pub(crate) async fn receive(&mut self) -> Option<PendingIngress> {
        self.receiver.recv().await.map(|command| PendingIngress {
            command,
            prepared: None,
        })
    }

    pub(crate) fn try_receive(&mut self) -> Option<PendingIngress> {
        self.receiver.try_recv().ok().map(|command| PendingIngress {
            command,
            prepared: None,
        })
    }

    fn authenticate_main(&self, caller: &IngressCaller) -> bool {
        matches!(caller, IngressCaller::Main(authority) if Arc::ptr_eq(authority, &self.authority))
    }

    fn authenticate_child<'a>(
        &self,
        caller: &'a IngressCaller,
    ) -> Option<&'a DelegatedConversationControl> {
        match caller {
            IngressCaller::Child { authority, control }
                if Arc::ptr_eq(authority, &self.authority) =>
            {
                Some(control.as_ref())
            }
            _ => None,
        }
    }

    fn target(&self, selector: &TargetSelector) -> Option<RegisteredTarget> {
        self.targets.get(selector).cloned()
    }

    fn owns_endpoint(&self, endpoint: &MailEndpoint) -> bool {
        self.main.as_ref() == Some(endpoint)
            || self
                .targets
                .values()
                .any(|target| &target.worker == endpoint)
    }

    pub(crate) fn authenticates_child_runtime(
        &self,
        identity: &ChildRuntimeIdentity,
        provenance: &plexmaton_session_store::collaboration::DelegatedConversationProvenance,
    ) -> bool {
        Arc::ptr_eq(&identity.authority, &self.authority)
            && &identity.endpoint == provenance.worker()
            && self.main.as_ref() == Some(provenance.delegator())
            && self
                .targets
                .values()
                .any(|target| target.worker == identity.endpoint)
    }

    pub(crate) fn bind_child_runtime(&mut self, identity: ChildRuntimeIdentity) {
        self.runtimes.insert(identity.endpoint, identity.runtime);
    }

    pub(crate) fn authenticates_session_source(&self, source: &CollaborationSessionSource) -> bool {
        if !Arc::ptr_eq(&source.authority, &self.authority)
            || source.journal.conversation_id() != &source.endpoint.conversation
            || !self
                .runtimes
                .get(&source.endpoint)
                .is_some_and(|runtime| Arc::ptr_eq(runtime, &source.runtime))
        {
            return false;
        }
        match source.role {
            SessionSourceRole::Main => self.main.as_ref() == Some(&source.endpoint),
            SessionSourceRole::Child => self
                .targets
                .values()
                .any(|target| target.worker == source.endpoint),
        }
    }

    fn resolve_artifacts(
        &self,
        endpoint: &MailEndpoint,
        selectors: &[crate::ArtifactSelector],
    ) -> Result<Vec<ArtifactReference>, CollaborationIngressRefusal> {
        selectors
            .iter()
            .map(|selector| {
                self.artifacts
                    .get(&(endpoint.clone(), selector.clone()))
                    .cloned()
                    .ok_or(CollaborationIngressRefusal::UnknownArtifact)
            })
            .collect()
    }
}

impl MainCollaborationIngress {
    pub(crate) async fn execute(
        &self,
        call_id: ToolCallId,
        request: CollaborationToolRequest,
        cancellation: CancellationToken,
    ) -> Result<CollaborationIngressResult, CollaborationIngressRefusal> {
        enqueue(
            &self.sender,
            call_id,
            IngressCaller::Main(Arc::clone(&self.authority)),
            request,
            cancellation,
            Arc::clone(&self.notify),
        )
        .await
    }
}

impl ChildCollaborationIngress {
    pub(crate) async fn execute(
        &self,
        call_id: ToolCallId,
        request: CollaborationToolRequest,
        cancellation: CancellationToken,
    ) -> Result<CollaborationIngressResult, CollaborationIngressRefusal> {
        enqueue(
            &self.sender,
            call_id,
            IngressCaller::Child {
                authority: Arc::clone(&self.authority),
                control: Arc::clone(&self.control),
            },
            request,
            cancellation,
            Arc::clone(&self.notify),
        )
        .await
    }
}

async fn enqueue(
    sender: &mpsc::Sender<IngressCommand>,
    call_id: ToolCallId,
    caller: IngressCaller,
    request: CollaborationToolRequest,
    cancellation: CancellationToken,
    notify: Arc<Notify>,
) -> Result<CollaborationIngressResult, CollaborationIngressRefusal> {
    if cancellation.is_cancelled() {
        return Err(CollaborationIngressRefusal::Cancelled);
    }
    let (reply, result) = oneshot::channel();
    match sender.try_send(IngressCommand {
        call_id,
        caller,
        request,
        reply,
    }) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            return Err(CollaborationIngressRefusal::Busy);
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            return Err(CollaborationIngressRefusal::Closed);
        }
    }
    notify.notify_one();
    tokio::select! {
        biased;
        result = result => result.unwrap_or(Err(CollaborationIngressRefusal::Closed)),
        () = cancellation.cancelled() => Err(CollaborationIngressRefusal::Cancelled),
    }
}

mod owner;
mod preparation;
mod provisioning;
mod source;

use source::SessionSourceRole;
pub(crate) use source::{ChildRuntimeIdentity, RuntimeCollaborationIdentity};
pub use source::{
    CollaborationArtifactSource, CollaborationRuntimeStamp, CollaborationSessionSource,
    MainRuntimeIdentity,
};

#[cfg(test)]
mod tests;
