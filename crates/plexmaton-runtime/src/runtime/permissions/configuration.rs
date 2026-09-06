//! The configuration reader is an external, blocking adapter owned by the coding Session.
use std::sync::Arc;

use plexmaton_agent::{PermissionChangeError, PermissionConfiguration, PermissionRule};

use super::CodingSessionPermissions;

/// Reloads one bounded project configuration through the composition's existing reader.
/// Called only before terminal ownership or in a retained worker, under the project store lock.
pub trait ProjectPermissionConfigurationSource: Send + Sync {
    /// Missing configuration is an empty source; every malformed or unreadable source is an error.
    fn load(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<PermissionConfiguration>, PermissionChangeError>;
}

impl CodingSessionPermissions {
    /// Installs user-owned rules as the coding Session's startup snapshot.
    pub fn with_user_rules(
        self,
        rules: Vec<PermissionRule>,
    ) -> Result<Self, PermissionChangeError> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| PermissionChangeError::Unavailable)?;
            let revision = state.snapshot().revision().clone();
            state.replace_rules(&revision, rules)?;
        }
        Ok(self)
    }

    /// Requires a live project reader in addition to the already configured personal store.
    pub fn with_project_configuration(
        mut self,
        source: Arc<dyn ProjectPermissionConfigurationSource>,
    ) -> Result<Self, PermissionChangeError> {
        if self.project.is_none() {
            return Err(PermissionChangeError::Unavailable);
        }
        self.configuration = Some(source);
        self.refresh(&|| false)?;
        Ok(self)
    }
}
