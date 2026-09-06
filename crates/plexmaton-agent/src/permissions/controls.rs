//! Native settings and revocation use the same bounded Session authority as pending approvals.
use super::*;
use plexmaton_core::{
    NativeFilePreset, PermissionAction, PermissionGrantView, PermissionIntent, PermissionScope,
    PermissionStateView,
};

impl PermissionSnapshot {
    /// Projects only the labels and identities needed by user controls; no policy is reconstructed there.
    #[must_use]
    pub fn control_view(&self) -> PermissionStateView {
        let native_files = if self.native_files.is_none() {
            NativeFilePreset::Unavailable
        } else if let Some(grant) = self
            .grants
            .iter()
            .find(|grant| grant.origin == PermissionGrantOrigin::NativeFilePreset)
        {
            NativeFilePreset::Enabled(grant.id.clone())
        } else {
            NativeFilePreset::Disabled
        };
        PermissionStateView {
            revision: self.revision.clone(),
            native_files,
            project: match self.project {
                ProjectPermissions::Disabled => plexmaton_core::ProjectPermissionSource::Disabled,
                ProjectPermissions::Ready { .. } => {
                    plexmaton_core::ProjectPermissionSource::Available
                }
                ProjectPermissions::Unavailable => {
                    plexmaton_core::ProjectPermissionSource::Unavailable
                }
            },
            configuration: match &self.project {
                ProjectPermissions::Ready {
                    configuration: Some(config),
                    trusted_config,
                    ..
                } => Some(plexmaton_core::ProjectConfigurationView {
                    fingerprint: config.fingerprint(),
                    rules: config
                        .rules()
                        .iter()
                        .map(|rule| plexmaton_core::PermissionRuleView {
                            action: rule.action,
                            label: rule.matcher.label(),
                        })
                        .collect(),
                    trusted: *trusted_config == Some(config.fingerprint()),
                }),
                _ => None,
            },
            trusted_config: match &self.project {
                ProjectPermissions::Ready { trusted_config, .. } => *trusted_config,
                _ => None,
            },
            grants: self
                .grants
                .iter()
                .map(|grant| PermissionGrantView {
                    id: grant.id.clone(),
                    scope: PermissionScope::Session,
                    label: grant.matcher.label(),
                })
                .chain(
                    self.project_grants()
                        .iter()
                        .map(|grant| PermissionGrantView {
                            id: grant.id.clone(),
                            scope: PermissionScope::Project,
                            label: grant.matcher.label(),
                        }),
                )
                .collect(),
        }
    }
}

impl PermissionMatcher {
    /// Scope copy comes from trusted matchers, not from tool result or assistant text.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::NativeFileChanges { .. } => {
                "Native create/edit; excludes agent controls and Git metadata".to_owned()
            }
            Self::CommandPrefix { prefix, .. } => format!("Command prefix: {} …", prefix.label()),
            Self::ExactCommand { command, .. } => format!("Exact command: {:?}", command.source()),
        }
    }
}

impl SessionPermissions {
    /// Applies a reviewed native setting or revocation without creating Conversation history.
    pub fn apply_control(
        &mut self,
        intent: &PermissionIntent,
    ) -> Result<(), PermissionChangeError> {
        match &intent.action {
            PermissionAction::TrustProjectConfiguration(_)
            | PermissionAction::RevokeProjectTrust => Err(PermissionChangeError::Unavailable),
            PermissionAction::EnableNativeFiles => self.enable_native_files(&intent.expected),
            PermissionAction::Revoke(id) => self.revoke(&intent.expected, id),
        }
    }

    fn enable_native_files(
        &mut self,
        expected: &PermissionRevision,
    ) -> Result<(), PermissionChangeError> {
        self.check_revision(expected)?;
        if self
            .snapshot
            .grants
            .iter()
            .any(|grant| grant.origin == PermissionGrantOrigin::NativeFilePreset)
        {
            return Ok(());
        }
        if self.snapshot.grants.len() >= MAX_PERMISSION_ENTRIES {
            return Err(PermissionChangeError::Capacity);
        }
        let (create, edit) = self
            .snapshot
            .native_files
            .clone()
            .ok_or(PermissionChangeError::Unavailable)?;
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
            id,
            matcher: PermissionMatcher::NativeFileChanges { create, edit },
            origin: PermissionGrantOrigin::NativeFilePreset,
        });
        Ok(())
    }
}
