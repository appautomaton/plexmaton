//! Trusted tool admission facts and the first stateless approval policy.
//!
//! A provider only says which name and raw arguments the model emitted. Whoever owns the trusted
//! tool catalog turns that request into [`AdmittedToolCall`]; the loop never infers authority from
//! the model's name or prose (APV-1 and APV-2).

use crate::{PermissionSnapshot, PermissionSubject};
use std::{num::NonZeroU64, sync::Arc};

use plexmaton_core::{ToolCallId, ToolCapability, ToolDefinitionId, ToolDetail};
use serde::{Deserialize, Serialize};

use crate::tools::{ToolCall, detail_fits_text_bound};

/// Maximum raw JSON argument bytes accepted from one model tool call.
pub const MAX_REQUESTED_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;
/// Maximum canonical argument bytes one admitted call may retain in turn state.
///
/// The extra KiB is reserved for trusted catalogs that replace raw syntax with bounded canonical
/// structure; it does not widen the provider input boundary.
pub const MAX_ADMITTED_ARGUMENT_BYTES: usize = MAX_REQUESTED_TOOL_ARGUMENT_BYTES + 1024;

/// Maximum concrete operation detail shown in an approval request.
pub const MAX_APPROVAL_DETAIL_BYTES: usize = 1024;

/// A finite, canonical set of capabilities.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapabilitySet(Vec<ToolCapability>);

impl CapabilitySet {
    /// Sorts and de-duplicates a capability collection.
    #[must_use]
    pub fn new(capabilities: impl IntoIterator<Item = ToolCapability>) -> Self {
        let mut capabilities: Vec<_> = capabilities.into_iter().collect();
        capabilities.sort_unstable();
        capabilities.dedup();
        Self(capabilities)
    }

    /// Iterates the set in stable capability order.
    pub fn iter(&self) -> impl Iterator<Item = ToolCapability> + '_ {
        self.0.iter().copied()
    }

    /// Whether this set shares at least one capability with `other`.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.0.iter().any(|capability| other.0.contains(capability))
    }

    pub(crate) fn to_vec(&self) -> Vec<ToolCapability> {
        self.0.clone()
    }
}

/// Monotonic revision of one trusted tool definition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ToolDefinitionRevision(NonZeroU64);

impl ToolDefinitionRevision {
    /// Creates a revision; zero is not a published definition revision.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the numeric revision.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Why a trusted catalog refused a model tool request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionRefusal {
    /// No registered definition owns the requested name.
    UnknownTool,
    /// The raw arguments did not satisfy the definition's schema or hard guards.
    InvalidArguments,
    /// The definition exists but its executor is unavailable.
    DefinitionUnavailable,
    /// A trusted observation or absence condition no longer holds.
    StalePrecondition,
    /// Exact source text did not occur in the observed region.
    SourceMismatch,
    /// Exact source text did not identify one unique target.
    AmbiguousTarget,
    /// Individually valid arguments conflict when applied as one operation.
    ConflictingArguments,
    /// Admission work was cancelled before it could publish a trusted call.
    Cancelled,
}

/// Why an admitted-call constructor rejected catalog output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmittedCallError {
    /// Canonical arguments exceeded the turn-state bound.
    ArgumentsTooLarge,
    /// Approval detail exceeded the presentation bound.
    DetailTooLarge,
    /// Canonical invocation presentation exceeded the retained text bound.
    PresentationTooLarge,
}

/// One exact loop-issued request a trusted catalog may consume to resolve admission (APV-1).
///
/// Callers cannot construct a ticket from an arbitrary [`ToolCall`]. Holding an already admitted
/// call therefore does not grant the ability to mint another call with changed canonical intent.
#[derive(Debug, Eq, PartialEq)]
pub struct AdmissionRequest {
    requested: ToolCall,
    subject: Box<PermissionSubject>,
}

impl AdmissionRequest {
    pub(crate) fn new(requested: ToolCall) -> Self {
        Self {
            requested,
            subject: Box::new(PermissionSubject::Opaque),
        }
    }

    /// Adds catalog-validated permission facts to this non-cloneable admission ticket.
    #[must_use]
    pub fn with_permission_subject(mut self, subject: PermissionSubject) -> Self {
        self.subject = Box::new(subject);
        self
    }

    /// Exact untrusted call the catalog must parse and answer.
    #[must_use]
    pub const fn requested(&self) -> &ToolCall {
        &self.requested
    }

    /// Consumes this loop-issued ticket and freezes trusted catalog facts into one admitted call.
    pub fn admit(
        self,
        definition_id: ToolDefinitionId,
        definition_revision: ToolDefinitionRevision,
        capabilities: impl IntoIterator<Item = ToolCapability>,
        canonical_arguments: String,
        detail: String,
        invocation: Option<ToolDetail>,
    ) -> Result<AdmissionOutcome, AdmittedCallError> {
        AdmittedToolCall::new(
            self.requested,
            definition_id,
            definition_revision,
            capabilities,
            canonical_arguments,
            detail,
            invocation,
        )
        .map(|mut call| {
            call.subject = self.subject;
            AdmissionOutcome::Admitted(call)
        })
    }

    /// Consumes this loop-issued ticket as a typed refusal.
    #[must_use]
    pub fn refuse(self, reason: AdmissionRefusal) -> AdmissionOutcome {
        AdmissionOutcome::Refused {
            call_id: self.requested.call_id,
            reason,
        }
    }
}

/// One immutable call after trusted parsing and canonicalization.
///
/// The constructor is intentionally not public; only consuming a loop-issued
/// [`AdmissionRequest`] can produce this value outside the loop crate.
///
/// ```compile_fail
/// let _constructor = plexmaton_agent::AdmittedToolCall::new;
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedToolCall {
    subject: Box<PermissionSubject>,
    requested: ToolCall,
    definition_id: ToolDefinitionId,
    definition_revision: ToolDefinitionRevision,
    capabilities: CapabilitySet,
    canonical_arguments: String,
    detail: String,
    invocation: Option<ToolDetail>,
}

impl AdmittedToolCall {
    /// Builds the value returned by a trusted catalog after enforcing its retained-size bounds.
    pub(crate) fn new(
        requested: ToolCall,
        definition_id: ToolDefinitionId,
        definition_revision: ToolDefinitionRevision,
        capabilities: impl IntoIterator<Item = ToolCapability>,
        canonical_arguments: String,
        detail: String,
        invocation: Option<ToolDetail>,
    ) -> Result<Self, AdmittedCallError> {
        if canonical_arguments.len() > MAX_ADMITTED_ARGUMENT_BYTES {
            return Err(AdmittedCallError::ArgumentsTooLarge);
        }
        if detail.len() > MAX_APPROVAL_DETAIL_BYTES {
            return Err(AdmittedCallError::DetailTooLarge);
        }
        if invocation
            .as_ref()
            .is_some_and(|detail| !detail_fits_text_bound(detail))
        {
            return Err(AdmittedCallError::PresentationTooLarge);
        }
        Ok(Self {
            subject: Box::new(PermissionSubject::Opaque),
            requested,
            definition_id,
            definition_revision,
            capabilities: CapabilitySet::new(capabilities),
            canonical_arguments,
            detail,
            invocation,
        })
    }

    /// Reusable facts issued by the trusted catalog, separate from rendered invocation detail.
    #[must_use]
    pub const fn permission_subject(&self) -> &PermissionSubject {
        &self.subject
    }

    /// The exact request this admitted call answers.
    #[must_use]
    pub const fn requested(&self) -> &ToolCall {
        &self.requested
    }

    /// Stable trusted definition identity.
    #[must_use]
    pub const fn definition_id(&self) -> &ToolDefinitionId {
        &self.definition_id
    }

    /// Trusted definition revision pinned by this call.
    #[must_use]
    pub const fn definition_revision(&self) -> ToolDefinitionRevision {
        self.definition_revision
    }

    /// Canonical capabilities policy evaluates.
    #[must_use]
    pub const fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    /// Canonical arguments the executor receives instead of the model's raw text.
    #[must_use]
    pub fn canonical_arguments(&self) -> &str {
        &self.canonical_arguments
    }

    /// Bounded concrete-operation detail shown to the user.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Bounded semantic description of the canonical invocation, when safe to expose.
    #[must_use]
    pub const fn invocation(&self) -> Option<&ToolDetail> {
        self.invocation.as_ref()
    }
}

/// Result of one explicit admission effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdmissionOutcome {
    /// The catalog accepted and canonicalized the call.
    Admitted(AdmittedToolCall),
    /// The catalog refused the request before policy or execution.
    Refused {
        /// Exact model call being answered.
        call_id: ToolCallId,
        /// Typed reason; presentation adapters may render it but never match its text.
        reason: AdmissionRefusal,
    },
}

/// Pure policy result over an admitted call.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    /// Run without asking the user.
    Allow,
    /// Park this call in turn state and ask the user.
    RequireApproval,
    /// Never run; approval cannot override this result.
    Forbidden,
    /// A required permission source could not be validated; no decision can authorize a call.
    Unavailable,
}

/// Stateless policy over typed capabilities (APV-2).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalPolicy {
    approval_required: CapabilitySet,
    forbidden: CapabilitySet,
    snapshot: Option<Arc<PermissionSnapshot>>,
    fallback_ask: CapabilitySet,
}

impl ApprovalPolicy {
    /// Creates a policy. A capability present in both sets is forbidden.
    #[must_use]
    pub fn new(approval_required: CapabilitySet, forbidden: CapabilitySet) -> Self {
        Self {
            approval_required,
            forbidden,
            snapshot: None,
            fallback_ask: CapabilitySet::default(),
        }
    }

    /// Decides an admitted call without I/O or presentation state (APV-2).
    #[must_use]
    pub fn decide(&self, call: &AdmittedToolCall) -> PolicyDecision {
        self.evaluate(call).decision()
    }

    fn evaluate<'a>(&'a self, call: &AdmittedToolCall) -> crate::permissions::PolicyMatch<'a> {
        use crate::permissions::PolicyMatch;
        if call.capabilities.intersects(&self.forbidden) {
            return PolicyMatch::Capability(PolicyDecision::Forbidden);
        }
        let explicit_ask = call.capabilities.intersects(&self.approval_required);
        let fallback = if explicit_ask || call.capabilities.intersects(&self.fallback_ask) {
            PolicyDecision::RequireApproval
        } else {
            PolicyDecision::Allow
        };
        self.snapshot.as_ref().map_or_else(
            || {
                if explicit_ask {
                    PolicyMatch::Capability(fallback)
                } else {
                    PolicyMatch::Fallback(fallback)
                }
            },
            |view| view.evaluate(call, fallback, explicit_ask),
        )
    }

    pub(crate) fn audit(
        &self,
        call: &AdmittedToolCall,
        user: Option<crate::PermissionUserDecision>,
    ) -> crate::PermissionDecisionAudit {
        crate::PermissionDecisionAudit::new(
            self.snapshot.as_deref(),
            call,
            self.evaluate(call).into(),
            user,
        )
    }

    /// Replaces the derived immutable permission view; the Session owner retains mutation authority.
    pub fn use_snapshot(&mut self, snapshot: Arc<PermissionSnapshot>) {
        self.snapshot = Some(snapshot);
    }

    pub(crate) fn remember_offer(
        &self,
        call: &AdmittedToolCall,
    ) -> Option<crate::permissions::PendingPermissionOffer> {
        let snapshot = self.snapshot.as_ref()?;
        if !self.remember_is_effective(snapshot, call) {
            return None;
        }
        snapshot.remember_offer(call)
    }

    pub(crate) fn approval_reason(
        &self,
        call: &AdmittedToolCall,
    ) -> plexmaton_core::ApprovalReason {
        use plexmaton_core::ApprovalReason;
        if call.capabilities.intersects(&self.approval_required)
            || self
                .snapshot
                .as_ref()
                .is_some_and(|view| !view.permits_remembering(call))
        {
            return ApprovalReason::ExplicitAsk;
        }
        match call.permission_subject() {
            PermissionSubject::NativeFileChange(_) => ApprovalReason::NativeFileChange,
            PermissionSubject::Command { .. } => ApprovalReason::CommandExecution,
            PermissionSubject::Opaque => ApprovalReason::PermissionRequired,
        }
    }

    pub(crate) fn offer_is_current(
        &self,
        offer: &crate::permissions::PendingPermissionOffer,
    ) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.revision() == &offer.revision)
    }

    pub(crate) fn remember_is_effective(
        &self,
        snapshot: &PermissionSnapshot,
        call: &AdmittedToolCall,
    ) -> bool {
        !call.capabilities.intersects(&self.forbidden)
            && !call.capabilities.intersects(&self.approval_required)
            && snapshot.permits_remembering(call)
    }
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            approval_required: CapabilitySet::default(),
            forbidden: CapabilitySet::default(),
            snapshot: None,
            fallback_ask: CapabilitySet::new([
                ToolCapability::FileWrite,
                ToolCapability::ProcessSpawn,
            ]),
        }
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{ToolCallId, ToolCapability, ToolDefinitionId, ToolDetail};

    use super::{
        AdmittedCallError, AdmittedToolCall, ApprovalPolicy, CapabilitySet,
        MAX_ADMITTED_ARGUMENT_BYTES, MAX_APPROVAL_DETAIL_BYTES, PolicyDecision,
        ToolDefinitionRevision,
    };
    use crate::ToolCall;

    fn call(capabilities: impl IntoIterator<Item = ToolCapability>) -> AdmittedToolCall {
        AdmittedToolCall::new(
            ToolCall {
                call_id: ToolCallId::new("call-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                name: "fixture".to_owned(),
                arguments: "{}".to_owned(),
            },
            ToolDefinitionId::new("fixture-v1").unwrap_or_else(|error| panic!("fixture: {error}")),
            ToolDefinitionRevision::new(1).unwrap_or_else(|| panic!("fixture revision")),
            capabilities,
            "{}".to_owned(),
            "fixture operation".to_owned(),
            None,
        )
        .unwrap_or_else(|error| panic!("fixture admitted call: {error:?}"))
    }

    #[test]
    fn capabilities_are_a_canonical_set() {
        let set = CapabilitySet::new([
            ToolCapability::ProcessSpawn,
            ToolCapability::FileRead,
            ToolCapability::FileRead,
        ]);

        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [ToolCapability::FileRead, ToolCapability::ProcessSpawn]
        );
    }

    #[test]
    fn policy_uses_capabilities_and_forbidden_wins() {
        let default = ApprovalPolicy::default();
        assert_eq!(
            default.decide(&call([ToolCapability::FileRead])),
            PolicyDecision::Allow
        );
        assert_eq!(
            default.decide(&call([ToolCapability::FileWrite])),
            PolicyDecision::RequireApproval
        );

        let forbidden_write = ApprovalPolicy::new(
            CapabilitySet::new([ToolCapability::FileWrite]),
            CapabilitySet::new([ToolCapability::FileWrite]),
        );
        assert_eq!(
            forbidden_write.decide(&call([ToolCapability::FileWrite])),
            PolicyDecision::Forbidden
        );
    }

    #[test]
    fn admitted_state_is_bounded_before_the_loop_can_retain_it() {
        let base = call([]);
        let request = base.requested().clone();
        let definition = base.definition_id().clone();
        let revision = base.definition_revision();

        assert_eq!(MAX_ADMITTED_ARGUMENT_BYTES, 65 * 1024);
        assert!(
            AdmittedToolCall::new(
                request.clone(),
                definition.clone(),
                revision,
                [],
                "x".repeat(MAX_ADMITTED_ARGUMENT_BYTES),
                String::new(),
                None,
            )
            .is_ok()
        );
        assert_eq!(
            AdmittedToolCall::new(
                request.clone(),
                definition.clone(),
                revision,
                [],
                "x".repeat(MAX_ADMITTED_ARGUMENT_BYTES + 1),
                String::new(),
                None,
            ),
            Err(AdmittedCallError::ArgumentsTooLarge)
        );
        assert_eq!(
            AdmittedToolCall::new(
                request.clone(),
                definition.clone(),
                revision,
                [],
                String::new(),
                "x".repeat(MAX_APPROVAL_DETAIL_BYTES + 1),
                None,
            ),
            Err(AdmittedCallError::DetailTooLarge)
        );
        assert_eq!(
            AdmittedToolCall::new(
                request,
                definition,
                revision,
                [],
                String::new(),
                String::new(),
                Some(ToolDetail::Text {
                    source: "x".repeat(crate::MAX_TOOL_PRESENTATION_TEXT_BYTES + 1),
                    omitted_bytes: 0,
                }),
            ),
            Err(AdmittedCallError::PresentationTooLarge)
        );
    }
}
