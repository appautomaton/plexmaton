//! Runtime-sealed identity and immutable projection sources.

use std::sync::Arc;

use plexmaton_agent::collaboration::MailEndpoint;
use plexmaton_agent::{ArtifactAnnouncementOrigin, ConversationJournal};
use plexmaton_core::HeadName;

use super::{ChildCollaborationIngress, IngressAuthority, MainCollaborationIngress};

/// Process-local identity unique to one `LiveRuntime` instance.
pub(crate) struct RuntimeCollaborationIdentity;

/// Opaque process-local identity for one exact `LiveRuntime` instance.
///
/// The value can be compared and retained, but it cannot be constructed outside the runtime. It
/// keeps a composition root from projecting owner activity into a replacement runtime that happens
/// to reopen the same durable conversation.
#[derive(Clone)]
pub struct CollaborationRuntimeStamp(Arc<RuntimeCollaborationIdentity>);

impl PartialEq for CollaborationRuntimeStamp {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CollaborationRuntimeStamp {}

impl RuntimeCollaborationIdentity {
    pub(crate) fn fresh() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl CollaborationRuntimeStamp {
    pub(crate) fn new(identity: Arc<RuntimeCollaborationIdentity>) -> Self {
        Self(identity)
    }
}

/// Proof that one user-owned root carries this Main capability and exact runtime identity.
pub struct MainRuntimeIdentity {
    pub(super) endpoint: MailEndpoint,
    pub(super) authority: Arc<IngressAuthority>,
    pub(super) runtime: Arc<RuntimeCollaborationIdentity>,
}

impl MainRuntimeIdentity {
    /// Which endpoint the root is, so its composition root can read the mail addressed to it.
    ///
    /// Binding consumes this proof, and a CMP-1 snapshot is keyed by endpoint, so the one caller
    /// that owns both has to take the address before it gives the capability away.
    #[must_use]
    pub const fn endpoint(&self) -> &MailEndpoint {
        &self.endpoint
    }
}

pub(crate) struct ChildRuntimeIdentity {
    pub(super) endpoint: MailEndpoint,
    pub(super) authority: Arc<IngressAuthority>,
    pub(super) runtime: Arc<RuntimeCollaborationIdentity>,
}

/// Immutable artifact facts sealed by their exact live runtime.
pub struct CollaborationArtifactSource {
    pub(super) endpoint: MailEndpoint,
    pub(super) selected: Vec<ArtifactAnnouncementOrigin>,
    pub(super) retained: Vec<ArtifactAnnouncementOrigin>,
    pub(super) authority: Arc<IngressAuthority>,
    pub(super) runtime: Arc<RuntimeCollaborationIdentity>,
}

/// Selected session snapshot sealed by the exact live runtime and collaboration capability.
pub struct CollaborationSessionSource {
    pub(super) endpoint: MailEndpoint,
    pub(super) journal: ConversationJournal,
    pub(super) head: HeadName,
    pub(super) role: SessionSourceRole,
    pub(super) authority: Arc<IngressAuthority>,
    pub(super) runtime: Arc<RuntimeCollaborationIdentity>,
}

#[derive(Clone, Copy)]
pub(super) enum SessionSourceRole {
    Main,
    Child,
}

impl CollaborationSessionSource {
    fn new(
        endpoint: MailEndpoint,
        journal: ConversationJournal,
        head: HeadName,
        role: SessionSourceRole,
        authority: Arc<IngressAuthority>,
        runtime: Arc<RuntimeCollaborationIdentity>,
    ) -> Self {
        Self {
            endpoint,
            journal,
            head,
            role,
            authority,
            runtime,
        }
    }

    pub(crate) fn into_parts(self) -> (MailEndpoint, ConversationJournal, HeadName) {
        (self.endpoint, self.journal, self.head)
    }
}

impl CollaborationArtifactSource {
    fn new(
        endpoint: MailEndpoint,
        selected: Vec<ArtifactAnnouncementOrigin>,
        retained: Vec<ArtifactAnnouncementOrigin>,
        authority: Arc<IngressAuthority>,
        runtime: Arc<RuntimeCollaborationIdentity>,
    ) -> Self {
        Self {
            endpoint,
            selected,
            retained,
            authority,
            runtime,
        }
    }
}

impl MainCollaborationIngress {
    pub(crate) fn identify(
        &self,
        endpoint: MailEndpoint,
        runtime: Arc<RuntimeCollaborationIdentity>,
    ) -> MainRuntimeIdentity {
        MainRuntimeIdentity {
            endpoint,
            authority: Arc::clone(&self.authority),
            runtime,
        }
    }

    pub(crate) fn artifact_source(
        &self,
        endpoint: MailEndpoint,
        selected: Vec<ArtifactAnnouncementOrigin>,
        retained: Vec<ArtifactAnnouncementOrigin>,
        runtime: Arc<RuntimeCollaborationIdentity>,
    ) -> CollaborationArtifactSource {
        CollaborationArtifactSource::new(
            endpoint,
            selected,
            retained,
            Arc::clone(&self.authority),
            runtime,
        )
    }

    pub(crate) fn session_source(
        &self,
        endpoint: MailEndpoint,
        journal: ConversationJournal,
        head: HeadName,
        runtime: Arc<RuntimeCollaborationIdentity>,
    ) -> CollaborationSessionSource {
        CollaborationSessionSource::new(
            endpoint,
            journal,
            head,
            SessionSourceRole::Main,
            Arc::clone(&self.authority),
            runtime,
        )
    }
}

impl ChildCollaborationIngress {
    pub(crate) fn runtime_identity(
        &self,
        endpoint: MailEndpoint,
        runtime: Arc<RuntimeCollaborationIdentity>,
    ) -> Option<ChildRuntimeIdentity> {
        (&endpoint == self.worker()).then(|| ChildRuntimeIdentity {
            endpoint,
            authority: Arc::clone(&self.authority),
            runtime,
        })
    }

    pub(crate) fn artifact_source(
        &self,
        endpoint: MailEndpoint,
        selected: Vec<ArtifactAnnouncementOrigin>,
        retained: Vec<ArtifactAnnouncementOrigin>,
        runtime: Arc<RuntimeCollaborationIdentity>,
    ) -> Option<CollaborationArtifactSource> {
        (&endpoint == self.worker()).then(|| {
            CollaborationArtifactSource::new(
                endpoint,
                selected,
                retained,
                Arc::clone(&self.authority),
                runtime,
            )
        })
    }

    pub(crate) fn session_source(
        &self,
        endpoint: MailEndpoint,
        journal: ConversationJournal,
        head: HeadName,
        runtime: Arc<RuntimeCollaborationIdentity>,
    ) -> Option<CollaborationSessionSource> {
        (&endpoint == self.worker()).then(|| {
            CollaborationSessionSource::new(
                endpoint,
                journal,
                head,
                SessionSourceRole::Child,
                Arc::clone(&self.authority),
                runtime,
            )
        })
    }
}
