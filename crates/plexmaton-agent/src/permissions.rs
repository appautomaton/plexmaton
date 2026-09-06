//! Bounded permission authority, separate from any Conversation journal (PER-1–PER-4).

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use plexmaton_core::{CodingSessionId, ToolDefinitionId};
pub use plexmaton_core::{PermissionChangeError, PermissionGrantId, PermissionRevision};

use crate::{AdmittedToolCall, ApprovalPolicy, ToolDefinitionRevision};

mod command;
pub use command::{CommandPrefix, CommandSyntax, LiteralCommand, LiteralShell, PrefixUnavailable};
mod audit;
mod evaluation;
pub use audit::{
    PermissionDecisionAudit, PermissionEvidence, PermissionProjectAudit, PermissionUserDecision,
};
pub(crate) use evaluation::PolicyMatch;
mod configuration;
mod controls;
pub use configuration::{PermissionConfiguration, PermissionRuleSource};
mod preparation;
mod project;
pub use project::ProjectPermissions;
mod subject;
pub(crate) use preparation::PendingPermissionOffer;
pub use preparation::{
    PermissionPreparationOutcome, PermissionPreparationRequest, PreparedPermission,
    ToolAuthorization,
};
pub use subject::{CommandPermission, NativeFileChange, PermissionSubject};

/// Hard count bound shared by temporary grants and configured rules.
pub const MAX_PERMISSION_ENTRIES: usize = 128;

/// Identity and revision of an explicitly selected trusted definition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDefinition {
    id: ToolDefinitionId,
    revision: ToolDefinitionRevision,
}

impl PermissionDefinition {
    /// Pins reviewed catalog identity; a display name cannot satisfy this binding.
    #[must_use]
    pub const fn new(id: ToolDefinitionId, revision: ToolDefinitionRevision) -> Self {
        Self { id, revision }
    }

    pub(crate) fn matches(&self, call: &AdmittedToolCall) -> bool {
        self.id == *call.definition_id() && self.revision == call.definition_revision()
    }
}

/// Reusable scopes evaluated only against trusted admission facts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionMatcher {
    /// The native create/edit preset, with both definitions explicitly pinned.
    NativeFileChanges {
        /// Trusted create definition.
        create: PermissionDefinition,
        /// Trusted edit definition.
        edit: PermissionDefinition,
    },
    /// Literal argv prefix bound to execution context; every operation must be covered.
    CommandPrefix {
        /// Trusted command definition.
        definition: PermissionDefinition,
        /// Literal argument prefix with captured execution context.
        prefix: CommandPrefix,
    },
    /// Exact source, including syntax unsupported by prefix analysis.
    ExactCommand {
        /// Trusted command definition.
        definition: PermissionDefinition,
        /// Admitted command facts, including fixed root and captured environment identity.
        command: CommandPermission,
    },
}

impl PermissionMatcher {
    /// Tests the entire admitted operation; presentation content is deliberately unavailable here.
    #[must_use]
    pub fn matches(&self, call: &AdmittedToolCall) -> bool {
        match (self, call.permission_subject()) {
            (
                Self::NativeFileChanges { create, edit },
                PermissionSubject::NativeFileChange(change),
            ) => {
                change.is_project_file()
                    && match change.operation() {
                        subject::FileChangeOperation::Create => create.matches(call),
                        subject::FileChangeOperation::Edit => edit.matches(call),
                    }
            }
            (
                Self::ExactCommand {
                    definition,
                    command,
                },
                PermissionSubject::Command {
                    command: actual, ..
                },
            ) => definition.matches(call) && command == actual,
            (
                Self::CommandPrefix { definition, prefix },
                PermissionSubject::Command {
                    command,
                    syntax: CommandSyntax::Literal(literal),
                },
            ) => {
                definition.matches(call)
                    && prefix.context() == command.context()
                    && literal
                        .commands()
                        .iter()
                        .all(|operation| prefix.matches(operation))
            }
            _ => false,
        }
    }

    // Restrictive rules apply to any covered command, before whole-request Allow or exact grants.
    pub(crate) fn matches_rule(
        &self,
        call: &AdmittedToolCall,
        action: PermissionRuleAction,
    ) -> bool {
        if action != PermissionRuleAction::Allow
            && let Self::CommandPrefix { definition, prefix } = self
            && let PermissionSubject::Command {
                command,
                syntax: CommandSyntax::Literal(literal),
            } = call.permission_subject()
        {
            return definition.matches(call)
                && prefix.context() == command.context()
                && literal
                    .commands()
                    .iter()
                    .any(|operation| prefix.matches(operation));
        }
        self.matches(call)
    }
}

pub use plexmaton_core::PermissionRuleAction;

/// One trusted configured rule; source trust is enforced by the configuration adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionRule {
    /// Exact configuration source for inspection and historical decision provenance.
    pub source: PermissionRuleSource,
    /// The admitted operation scope.
    pub matcher: PermissionMatcher,
    /// Precedence-controlled action.
    pub action: PermissionRuleAction,
}

/// A remembered permission and its revocation identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionGrant {
    /// Stable identity used by revocation, never a list position.
    pub id: PermissionGrantId,
    /// Scope this grant authorizes.
    pub matcher: PermissionMatcher,
    /// Which explicit control created this grant; the native setting derives from this identity.
    pub origin: PermissionGrantOrigin,
}

/// A named setting has its own grant, independent of permissions remembered for individual calls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionGrantOrigin {
    /// A pending call's reviewed reusable scope.
    Approval,
    /// The explicit native create/edit setting.
    NativeFilePreset,
}

/// Immutable policy view published by the Session owner; agents cannot mutate its authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionSnapshot {
    revision: PermissionRevision,
    rules: Vec<PermissionRule>,
    grants: Vec<PermissionGrant>,
    native_files: Option<(PermissionDefinition, PermissionDefinition)>,
    project: ProjectPermissions,
}

impl PermissionSnapshot {
    /// Revision to echo when applying a reviewed permission change.
    #[must_use]
    pub const fn revision(&self) -> &PermissionRevision {
        &self.revision
    }

    /// Active temporary grants, in deterministic creation order.
    #[must_use]
    pub fn grants(&self) -> &[PermissionGrant] {
        &self.grants
    }

    pub(crate) fn remember_offer(&self, call: &AdmittedToolCall) -> Option<PendingPermissionOffer> {
        use plexmaton_core::{PermissionOfferId, PermissionScopes, RememberPermissionOffer};
        let (matcher, label, note) = match call.permission_subject() {
            PermissionSubject::NativeFileChange(_) => {
                let (create, edit) = self.native_files.as_ref()?;
                (
                    PermissionMatcher::NativeFileChanges {
                        create: create.clone(),
                        edit: edit.clone(),
                    },
                    "native create/edit; no controls/Git".to_owned(),
                    None,
                )
            }
            PermissionSubject::Command { command, syntax } => {
                let definition = PermissionDefinition::new(
                    call.definition_id().clone(),
                    call.definition_revision(),
                );
                match syntax.suggested_prefix(*command.context()) {
                    Some(prefix) => {
                        let label = format!("{} …; same cwd/environment", prefix.label());
                        let matcher = PermissionMatcher::CommandPrefix { definition, prefix };
                        (matcher, label, None)
                    }
                    None => (
                        PermissionMatcher::ExactCommand {
                            definition,
                            command: command.clone(),
                        },
                        "exact command; same cwd/environment".to_owned(),
                        Some(syntax.exact_only_reason().to_owned()),
                    ),
                }
            }
            PermissionSubject::Opaque => return None,
        };
        if !matcher.matches(call) || !self.permits_remembering(call) {
            return None;
        }
        Some(PendingPermissionOffer {
            display: RememberPermissionOffer {
                id: PermissionOfferId::new(self.revision.sequence()),
                label,
                note,
                scopes: if matches!(
                    self.project,
                    ProjectPermissions::Ready {
                        can_remember: true,
                        ..
                    }
                ) {
                    PermissionScopes::SessionAndProject
                } else {
                    PermissionScopes::Session
                },
            },
            revision: self.revision.clone(),
            matcher,
        })
    }

    pub(crate) fn permits_remembering(&self, call: &AdmittedToolCall) -> bool {
        !matches!(self.project, ProjectPermissions::Unavailable)
            && !self.effective_rules().any(|rule| {
                rule.action != PermissionRuleAction::Allow
                    && rule.matcher.matches_rule(call, rule.action)
            })
    }
}

/// The sole mutable owner of temporary permissions for one coding Session and workspace.
///
/// No journal decoder constructs this state. Its owner outlives replaced Conversation runtimes;
/// an immutable snapshot is a projection, not a second independently mutable permission list.
#[derive(Debug)]
pub struct SessionPermissions {
    snapshot: Arc<PermissionSnapshot>,
}

impl SessionPermissions {
    /// Creates empty, memory-only authority; callers supply a fresh coding Session identity.
    #[must_use]
    pub fn new(session: CodingSessionId) -> Self {
        Self {
            snapshot: Arc::new(PermissionSnapshot {
                revision: PermissionRevision::initial(session),
                rules: Vec::new(),
                grants: Vec::new(),
                native_files: None,
                project: ProjectPermissions::Disabled,
            }),
        }
    }

    /// Installs the catalog's explicit create/edit pair before publishing the Session owner.
    #[must_use]
    pub fn with_native_file_changes(
        mut self,
        create: PermissionDefinition,
        edit: PermissionDefinition,
    ) -> Self {
        Arc::make_mut(&mut self.snapshot).native_files = Some((create, edit));
        self
    }

    /// Shares the immutable current view without copying its scopes on every input.
    #[must_use]
    pub fn snapshot(&self) -> Arc<PermissionSnapshot> {
        Arc::clone(&self.snapshot)
    }

    /// Applies trusted rules as one bounded revision; source loading cannot partially replace them.
    pub fn replace_rules(
        &mut self,
        expected: &PermissionRevision,
        rules: Vec<PermissionRule>,
    ) -> Result<(), PermissionChangeError> {
        if rules.len() > MAX_PERMISSION_ENTRIES {
            return Err(PermissionChangeError::Capacity);
        }
        self.advance(expected)?.rules = rules;
        Ok(())
    }

    /// Remembers a scope only when it actually authorizes the admitted call under current policy.
    pub fn remember(
        &mut self,
        expected: &PermissionRevision,
        matcher: PermissionMatcher,
        call: &AdmittedToolCall,
        policy: &ApprovalPolicy,
    ) -> Result<PermissionGrantId, PermissionChangeError> {
        self.validate_remember(expected, &matcher, call, policy)?;
        if self.snapshot.grants.len() >= MAX_PERMISSION_ENTRIES {
            return Err(PermissionChangeError::Capacity);
        }
        let state = self.advance(expected)?;
        let id = PermissionGrantId::new(format!(
            "{}:{}",
            state.revision.session(),
            state.revision.sequence()
        ))
        .unwrap_or_else(|_| {
            unreachable!("owner and numeric revision form a nonempty grant identity")
        });
        state.grants.push(PermissionGrant {
            id: id.clone(),
            matcher,
            origin: PermissionGrantOrigin::Approval,
        });
        Ok(id)
    }

    /// Revokes one stable grant. Removing one source does not promise that no other rule allows it.
    pub fn revoke(
        &mut self,
        expected: &PermissionRevision,
        id: &PermissionGrantId,
    ) -> Result<(), PermissionChangeError> {
        self.check_revision(expected)?;
        let Some(index) = self
            .snapshot
            .grants
            .iter()
            .position(|grant| &grant.id == id)
        else {
            return Err(PermissionChangeError::NotFound);
        };
        self.advance(expected)?.grants.remove(index);
        Ok(())
    }

    fn check_revision(&self, expected: &PermissionRevision) -> Result<(), PermissionChangeError> {
        if expected == &self.snapshot.revision {
            Ok(())
        } else {
            Err(PermissionChangeError::StaleRevision)
        }
    }

    fn advance(
        &mut self,
        expected: &PermissionRevision,
    ) -> Result<&mut PermissionSnapshot, PermissionChangeError> {
        self.check_revision(expected)?;
        let next = expected.next().ok_or(PermissionChangeError::Capacity)?;
        let state = Arc::make_mut(&mut self.snapshot);
        state.revision = next;
        Ok(state)
    }
}

#[cfg(test)]
mod tests;
