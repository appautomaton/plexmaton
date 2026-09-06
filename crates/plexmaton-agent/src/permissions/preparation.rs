//! A remembered decision prepares authority before the Conversation's execution audit (PER-5).

use plexmaton_core::{ApprovalId, PermissionScope, RememberPermissionOffer};

use crate::{
    AdmittedToolCall, PermissionChangeError, PermissionGrantId, PermissionMatcher,
    PermissionRevision,
};

/// The private authoritative counterpart of one offered scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PendingPermissionOffer {
    pub(crate) display: RememberPermissionOffer,
    pub(crate) revision: PermissionRevision,
    pub(crate) matcher: PermissionMatcher,
}

/// Non-cloneable, loop-issued ticket for applying one reviewed remembered permission.
#[derive(Debug, Eq, PartialEq)]
pub struct PermissionPreparationRequest {
    approval_id: ApprovalId,
    admitted: AdmittedToolCall,
    offer: PendingPermissionOffer,
    scope: PermissionScope,
}

impl PermissionPreparationRequest {
    pub(crate) const fn new(
        approval_id: ApprovalId,
        admitted: AdmittedToolCall,
        offer: PendingPermissionOffer,
        scope: PermissionScope,
    ) -> Self {
        Self {
            approval_id,
            admitted,
            offer,
            scope,
        }
    }

    /// Exact pending approval to which preparation belongs.
    #[must_use]
    pub const fn approval_id(&self) -> &ApprovalId {
        &self.approval_id
    }
    /// Admitted operation for which the offered scope must remain effective.
    #[must_use]
    pub const fn admitted(&self) -> &AdmittedToolCall {
        &self.admitted
    }
    /// Immutable authoritative scope; the UI supplied only its offer identity.
    #[must_use]
    pub const fn matcher(&self) -> &PermissionMatcher {
        &self.offer.matcher
    }
    /// Reviewed permission revision that must still be current before mutation.
    #[must_use]
    pub const fn revision(&self) -> &PermissionRevision {
        &self.offer.revision
    }
    /// User-selected lifetime.
    #[must_use]
    pub const fn scope(&self) -> PermissionScope {
        self.scope
    }

    /// Consumes this ticket after the permission owner has accepted its mutation.
    #[must_use]
    pub fn prepared(self, grant: PermissionGrantId) -> PermissionPreparationOutcome {
        PermissionPreparationOutcome::Prepared(Box::new(PreparedPermission {
            approval_id: self.approval_id,
            admitted: self.admitted,
            offer: self.offer,
            scope: self.scope,
            grant,
        }))
    }

    /// Consumes this ticket as an observable failure without resolving its pending tool call.
    #[must_use]
    pub fn refused(self, reason: PermissionChangeError) -> PermissionPreparationOutcome {
        PermissionPreparationOutcome::Refused {
            approval_id: self.approval_id,
            reason,
        }
    }
}

/// Successful preparation, correlated by the consumed ticket rather than caller-supplied IDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedPermission {
    pub(crate) approval_id: ApprovalId,
    pub(crate) admitted: AdmittedToolCall,
    pub(crate) offer: PendingPermissionOffer,
    pub(crate) scope: PermissionScope,
    pub(crate) grant: PermissionGrantId,
}

impl PreparedPermission {
    /// Original call whose dependent execution still awaits the Conversation audit.
    #[must_use]
    pub fn call_id(&self) -> &plexmaton_core::ToolCallId {
        &self.admitted.requested().call_id
    }

    /// Applied authority retained even if cancellation prevents its Conversation audit (PER-6).
    #[must_use]
    pub const fn grant(&self) -> &PermissionGrantId {
        &self.grant
    }

    /// Lifetime of the already applied grant; this receipt itself confers no authority.
    #[must_use]
    pub const fn scope(&self) -> PermissionScope {
        self.scope
    }
}

/// Result of the permission owner's retained preparation operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PermissionPreparationOutcome {
    /// The exact reviewed scope was applied; execution still awaits JRN-7 acknowledgement.
    Prepared(Box<PreparedPermission>),
    /// Nothing authorized the dependent call; the pending request can be reviewed again.
    Refused {
        /// Exact request being answered.
        approval_id: ApprovalId,
        /// Typed failure surfaced to the approval controller.
        reason: PermissionChangeError,
    },
}

/// Why an execution effect may cross the runtime boundary after its audit is acknowledged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolAuthorization {
    /// Current configured/default/remembered policy must still allow this call at dispatch.
    Policy,
    /// The user approved this exact admitted call; an explicit Deny still blocks dispatch.
    Once,
    /// A just-prepared grant must remain effective at dispatch.
    Remembered {
        /// Applied grant retained for audit correlation and revocation.
        grant: PermissionGrantId,
        /// Which authority was changed; audit failure must report a durable Project partial outcome.
        scope: PermissionScope,
    },
}
