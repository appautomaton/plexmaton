//! Permission decisions and revisioned presentation contracts. These types hold no authority.
use crate::{CodingSessionId, PermissionGrantId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Revision of one coding Session's published policy view.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PermissionRevision {
    session: CodingSessionId,
    sequence: u64,
}

impl PermissionRevision {
    /// The empty policy view for a fresh explicit owner.
    #[must_use]
    pub const fn initial(session: CodingSessionId) -> Self {
        Self {
            session,
            sequence: 0,
        }
    }
    /// Session whose current authority must validate an intent.
    #[must_use]
    pub const fn session(&self) -> &CodingSessionId {
        &self.session
    }
    /// Monotonic revision within that owner.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }
    /// Advances without wrapping; only the authority owner decides whether this becomes current.
    #[must_use]
    pub fn next(&self) -> Option<Self> {
        Some(Self {
            session: self.session.clone(),
            sequence: self.sequence.checked_add(1)?,
        })
    }
}

/// Why the producer requires an individual approval.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalReason {
    /// Generic policy requirement, including historical requests without a detailed reason.
    #[default]
    PermissionRequired,
    /// An explicit Ask rule overrides grants and capability fallback.
    ExplicitAsk,
    /// No current permission allows this native file change.
    NativeFileChange,
    /// No current permission allows this shell command.
    CommandExecution,
}

/// Producer-issued identity of one remembered-permission offer within its pending approval.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PermissionOfferId(u64);

impl PermissionOfferId {
    /// Binds the offer to the Session policy revision; the enclosing approval supplies call identity.
    #[must_use]
    pub const fn new(revision: u64) -> Self {
        Self(revision)
    }
}

/// The lifetime chosen for an offered reusable permission.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionScope {
    /// The coding Session, until Plexmaton exits.
    Session,
    /// Personal project permissions, retained across application restarts.
    Project,
}

/// Available lifetimes are issued by the producer, never inferred by the UI.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionScopes {
    /// Memory-only authority is available.
    Session,
    /// Both memory-only and durable personal project authority are available.
    SessionAndProject,
}

impl PermissionScopes {
    /// Whether this exact offer can use the requested lifetime.
    #[must_use]
    pub const fn contains(self, scope: PermissionScope) -> bool {
        matches!(scope, PermissionScope::Session) || matches!(self, Self::SessionAndProject)
    }
}

/// Bounded presentation of a backend-validated reusable permission, not its authoritative matcher.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RememberPermissionOffer {
    /// Identity echoed with the decision.
    pub id: PermissionOfferId,
    /// Concrete scope description issued by the permission evaluator.
    pub label: String,
    /// Explanatory copy separate from the complete scope; short cards may omit this note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Lifetimes the current backend can apply.
    pub scopes: PermissionScopes,
}

/// One active grant projected for review or revocation; the matcher stays with the authority owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionGrantView {
    /// Stable identity echoed by revocation.
    pub id: PermissionGrantId,
    /// Lifetime and persistence boundary.
    pub scope: PermissionScope,
    /// Concrete, bounded scope description.
    pub label: String,
}

/// The native file-change setting derives from its named grant, never an independent boolean.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeFilePreset {
    /// The catalog cannot supply the required definition pair.
    Unavailable,
    /// The named setting has no active grant.
    Disabled,
    /// Revoking this exact grant turns the setting off; other policy sources may still allow changes.
    Enabled(PermissionGrantId),
}

/// Bounded, immutable display projection of the current permission owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionStateView {
    /// Revision every mutation must echo.
    pub revision: PermissionRevision,
    /// Named native create/edit setting.
    pub native_files: NativeFilePreset,
    /// Whether the external project source can supply current permission authority.
    pub project: ProjectPermissionSource,
    /// Complete current project rules for review before activation.
    pub configuration: Option<ProjectConfigurationView>,
    /// Existing personal trust can be withdrawn even after the project file is removed.
    pub trusted_config: Option<[u8; 32]>,
    /// Current grants in deterministic creation order.
    pub grants: Vec<PermissionGrantView>,
}

/// Availability of a personal project permission adapter, projected without authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectPermissionSource {
    /// This runtime has no project store configured.
    Disabled,
    /// The most recent read validated the whole source.
    Available,
    /// The configured source failed validation; execution requires a successful refresh.
    Unavailable,
}

/// A concrete user change; display labels and list positions never route mutations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PermissionAction {
    /// Enable native create/edit in memory for the current coding Session.
    EnableNativeFiles,
    /// Activate Allow rules only for these exact reviewed project configuration bytes.
    TrustProjectConfiguration([u8; 32]),
    /// Withdraw the current personal configuration trust.
    RevokeProjectTrust,
    /// Revoke one reviewed permission by stable identity.
    Revoke(PermissionGrantId),
}

/// A user-approved mutation tied to the exact view that was reviewed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionIntent {
    /// The owner rejects stale or foreign revisions without mutation.
    pub expected: PermissionRevision,
    /// The concrete setting or grant to change.
    pub action: PermissionAction,
}

/// The decisions APV-4 accepts for one pending approval.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// Permit only the admitted call named by the request.
    AllowOnce,
    /// Apply the backend-issued reusable permission before resolving this exact call.
    AllowAndRemember {
        /// Exact offer visible when the user decided.
        offer: PermissionOfferId,
        /// Explicitly selected grant lifetime.
        scope: PermissionScope,
    },
    /// Decline only the admitted call named by the request.
    Deny,
}

/// Typed refusal of a permission mutation; the prior state remains intact.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Error, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionChangeError {
    /// The requested backend could not complete preparation; no dependent call may run.
    #[error("permission preparation is unavailable; review current permissions before retrying")]
    Unavailable,
    /// A changed policy or another Session invalidated the reviewed view.
    #[error("permissions changed; review the current choices")]
    StaleRevision,
    /// The requested change exceeds the finite count or revision budget.
    #[error("permission capacity reached")]
    Capacity,
    /// The named permission is no longer present.
    #[error("permission is no longer active")]
    NotFound,
    /// An explicit Ask/Deny makes the proposed permission ineffective for the pending operation.
    #[error("an explicit rule prevents remembering this operation")]
    Ineffective,
}

/// Effect of an explicit rule. Deny precedes Ask, which precedes Allow and remembered grants.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRuleAction {
    /// Never authorize a matching operation.
    Deny,
    /// Require an individual decision even when a remembered scope also matches.
    Ask,
    /// Authorize an operation unless an explicit Deny or Ask matches.
    Allow,
}

/// Full semantic scope copy for reviewing configured rules; it is never executable authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionRuleView {
    /// Precedence-controlled effect of this scope.
    pub action: PermissionRuleAction,
    /// Complete catalog-issued scope description, before any viewport clipping.
    pub label: String,
}

/// Current project configuration and whether its exact bytes have personal trust.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectConfigurationView {
    /// Exact file fingerprint echoed by activation.
    pub fingerprint: [u8; 32],
    /// All rules in source order, including currently inactive Allow rules.
    pub rules: Vec<PermissionRuleView>,
    /// Whether the personal store trusts this exact current fingerprint.
    pub trusted: bool,
}

/// A personal Project grant acknowledged while its dependent call ended before execution.
/// This bounded receipt is feedback only and cannot restore authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedProjectPermission {
    /// The admitted call that did not start execution.
    pub call_id: crate::ToolCallId,
    /// Revocable identity of the already saved Project grant.
    pub grant: PermissionGrantId,
}
