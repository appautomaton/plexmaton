//! One evaluator supplies both the live decision and its bounded historical evidence (PER-9).

use plexmaton_core::PermissionScope;

use super::{
    PermissionGrant, PermissionRule, PermissionRuleAction, PermissionSnapshot, ProjectPermissions,
};
use crate::{AdmittedToolCall, PolicyDecision};

pub(crate) enum PolicyMatch<'a> {
    Fallback(PolicyDecision),
    Capability(PolicyDecision),
    Unavailable,
    Rule(&'a PermissionRule),
    Grant(&'a PermissionGrant, PermissionScope),
}

impl PolicyMatch<'_> {
    pub(crate) const fn decision(&self) -> PolicyDecision {
        match self {
            Self::Fallback(decision) | Self::Capability(decision) => *decision,
            Self::Unavailable => PolicyDecision::Unavailable,
            Self::Grant(..) => PolicyDecision::Allow,
            Self::Rule(rule) => match rule.action {
                PermissionRuleAction::Allow => PolicyDecision::Allow,
                PermissionRuleAction::Ask => PolicyDecision::RequireApproval,
                PermissionRuleAction::Deny => PolicyDecision::Forbidden,
            },
        }
    }
}

impl PermissionSnapshot {
    pub(crate) fn evaluate(
        &self,
        call: &AdmittedToolCall,
        fallback: PolicyDecision,
        explicit_ask: bool,
    ) -> PolicyMatch<'_> {
        if matches!(self.project, ProjectPermissions::Unavailable) {
            return PolicyMatch::Unavailable;
        }
        let matched = |action| {
            self.effective_rules()
                .find(|rule| rule.action == action && rule.matcher.matches_rule(call, action))
        };
        if let Some(rule) = matched(PermissionRuleAction::Deny) {
            return PolicyMatch::Rule(rule);
        }
        if explicit_ask {
            return PolicyMatch::Capability(PolicyDecision::RequireApproval);
        }
        if let Some(rule) =
            matched(PermissionRuleAction::Ask).or_else(|| matched(PermissionRuleAction::Allow))
        {
            return PolicyMatch::Rule(rule);
        }
        self.grants
            .iter()
            .map(|grant| (grant, PermissionScope::Session))
            .chain(
                self.project_grants()
                    .iter()
                    .map(|grant| (grant, PermissionScope::Project)),
            )
            .find(|(grant, _)| grant.matcher.matches(call))
            .map_or(PolicyMatch::Fallback(fallback), |(grant, scope)| {
                PolicyMatch::Grant(grant, scope)
            })
    }
}
