//! Immutable evidence at the reducer decision boundary. Decoding never installs authority.

use plexmaton_core::{
    ApprovalDecision, ApprovalId, ApprovalReason, PermissionRevision, PermissionScope,
    ProjectPermissionRevision,
};
use serde::{Deserialize, Serialize};

use super::evaluation::PolicyMatch;
use super::{
    PermissionDefinition, PermissionGrantId, PermissionMatcher, PermissionRuleAction,
    PermissionRuleSource, PermissionSnapshot, ProjectPermissions,
};
use crate::{AdmittedToolCall, PolicyDecision};

/// Required project source observed by this historical decision, without copying its grant list.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionProjectAudit {
    Disabled,
    Unavailable,
    Available { revision: ProjectPermissionRevision },
}

/// The winning source from the same evaluator that produced the policy decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "basis", rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionEvidence {
    Fallback {
        decision: PolicyDecision,
    },
    Capability {
        decision: PolicyDecision,
    },
    Unavailable,
    Rule {
        action: PermissionRuleAction,
        source: PermissionRuleSource,
        matcher: PermissionMatcher,
    },
    Grant {
        id: PermissionGrantId,
        scope: PermissionScope,
        matcher: PermissionMatcher,
    },
}

impl From<PolicyMatch<'_>> for PermissionEvidence {
    fn from(value: PolicyMatch<'_>) -> Self {
        match value {
            PolicyMatch::Fallback(decision) => Self::Fallback { decision },
            PolicyMatch::Capability(decision) => Self::Capability { decision },
            PolicyMatch::Unavailable => Self::Unavailable,
            PolicyMatch::Rule(rule) => Self::Rule {
                action: rule.action,
                source: rule.source.clone(),
                matcher: rule.matcher.clone(),
            },
            PolicyMatch::Grant(grant, scope) => Self::Grant {
                id: grant.id.clone(),
                scope,
                matcher: grant.matcher.clone(),
            },
        }
    }
}

/// An explicit user choice; absent when current policy released a waiting call automatically.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionUserDecision {
    pub approval_id: ApprovalId,
    pub decision: ApprovalDecision,
    pub reason: ApprovalReason,
    pub remembered: Option<PermissionGrantId>,
}

/// A bounded historical fact tied to its enclosing call, Conversation and head.
/// It precedes dependent effects, but does not claim that a later dispatch guard passed (PER-9).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDecisionAudit {
    pub definition: PermissionDefinition,
    pub command_context: Option<[u8; 32]>,
    pub revision: Option<PermissionRevision>,
    pub project: PermissionProjectAudit,
    pub evidence: PermissionEvidence,
    pub user: Option<PermissionUserDecision>,
}

impl PermissionDecisionAudit {
    pub(crate) fn new(
        snapshot: Option<&PermissionSnapshot>,
        call: &AdmittedToolCall,
        evidence: PermissionEvidence,
        user: Option<PermissionUserDecision>,
    ) -> Self {
        let project = match snapshot.map(PermissionSnapshot::project) {
            None | Some(ProjectPermissions::Disabled) => PermissionProjectAudit::Disabled,
            Some(ProjectPermissions::Unavailable) => PermissionProjectAudit::Unavailable,
            Some(ProjectPermissions::Ready { revision, .. }) => PermissionProjectAudit::Available {
                revision: revision.clone(),
            },
        };
        Self {
            definition: PermissionDefinition::new(
                call.definition_id().clone(),
                call.definition_revision(),
            ),
            command_context: match call.permission_subject() {
                super::PermissionSubject::Command { command, .. } => Some(*command.context()),
                _ => None,
            },
            revision: snapshot.map(|snapshot| snapshot.revision().clone()),
            project,
            evidence,
            user,
        }
    }
}
