//! Immutable projection of the external personal project source. The store owns its authority.

use plexmaton_core::ProjectPermissionRevision;

use super::{
    MAX_PERMISSION_ENTRIES, PermissionChangeError, PermissionGrant, PermissionSnapshot,
    SessionPermissions,
};

/// State of the required project source; an invalid source never becomes an empty allow list.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ProjectPermissions {
    /// No project adapter is configured (isolated or synthetic runtimes).
    #[default]
    Disabled,
    /// The most recently validated store snapshot. Dispatch must refresh it under the store lock.
    Ready {
        can_remember: bool,
        revision: ProjectPermissionRevision,
        grants: Vec<PermissionGrant>,
        configuration: Option<super::PermissionConfiguration>,
        trusted_config: Option<[u8; 32]>,
    },
    /// A configured source could not be validated; even Allow once must stop.
    Unavailable,
}

impl PermissionSnapshot {
    pub(super) fn effective_rules(&self) -> impl Iterator<Item = &super::PermissionRule> {
        let project = match &self.project {
            ProjectPermissions::Ready {
                configuration: Some(config),
                trusted_config,
                ..
            } => Some((config, *trusted_config == Some(config.fingerprint()))),
            _ => None,
        };
        self.rules
            .iter()
            .chain(project.into_iter().flat_map(|(config, trusted)| {
                config.rules().iter().filter(move |rule| {
                    trusted || rule.action != super::PermissionRuleAction::Allow
                })
            }))
    }

    /// External source state observed when this immutable view was published.
    #[must_use]
    pub const fn project(&self) -> &ProjectPermissions {
        &self.project
    }

    /// Project grants are a read projection and cannot be mutated as Session grants.
    #[must_use]
    pub fn project_grants(&self) -> &[PermissionGrant] {
        match &self.project {
            ProjectPermissions::Ready { grants, .. } => grants,
            ProjectPermissions::Disabled | ProjectPermissions::Unavailable => &[],
        }
    }

    /// Finds a grant by typed identity across the two explicitly owned sources.
    #[must_use]
    pub fn grant(&self, id: &super::PermissionGrantId) -> Option<&PermissionGrant> {
        self.grants
            .iter()
            .chain(self.project_grants())
            .find(|grant| &grant.id == id)
    }
}

impl SessionPermissions {
    /// Publishes a fresh project observation and invalidates prior offers only when it changed.
    pub fn observe_project(
        &mut self,
        project: ProjectPermissions,
    ) -> Result<(), PermissionChangeError> {
        if let ProjectPermissions::Ready { grants, .. } = &project
            && grants.len() > MAX_PERMISSION_ENTRIES
        {
            return Err(PermissionChangeError::Capacity);
        }
        if self.snapshot.project != project {
            let revision = self.snapshot.revision.clone();
            self.advance(&revision)?.project = project;
        }
        Ok(())
    }

    /// Validates a prepared project mutation while the adapter holds the shared store lock.
    pub fn validate_remember(
        &self,
        expected: &super::PermissionRevision,
        matcher: &super::PermissionMatcher,
        call: &crate::AdmittedToolCall,
        policy: &crate::ApprovalPolicy,
    ) -> Result<(), PermissionChangeError> {
        self.check_revision(expected)?;
        if !matcher.matches(call) || !policy.remember_is_effective(&self.snapshot, call) {
            return Err(PermissionChangeError::Ineffective);
        }
        Ok(())
    }
}
