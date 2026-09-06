//! Configured scopes are compiled by the trusted catalog; project activation is a separate fact.
use serde::{Deserialize, Serialize};

use super::{MAX_PERMISSION_ENTRIES, PermissionChangeError, PermissionRule};

/// Historical provenance of one compiled rule. This value does not confer authority on replay.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionRuleSource {
    /// Explicit policy supplied by a trusted runtime composition.
    Runtime,
    /// Rule in the user-owned startup configuration.
    UserConfiguration { fingerprint: [u8; 32], index: u16 },
    /// Rule in the current project-owned configuration; Allow requires matching personal trust.
    ProjectConfiguration { fingerprint: [u8; 32], index: u16 },
}

/// One complete bounded project configuration observation, before personal trust is applied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionConfiguration {
    fingerprint: [u8; 32],
    rules: Vec<PermissionRule>,
}

impl PermissionConfiguration {
    /// Tags a whole-source observation with its exact byte fingerprint; partial loads are refused.
    pub fn new(
        fingerprint: [u8; 32],
        mut rules: Vec<PermissionRule>,
    ) -> Result<Self, PermissionChangeError> {
        if rules.len() > MAX_PERMISSION_ENTRIES {
            return Err(PermissionChangeError::Capacity);
        }
        for (index, rule) in rules.iter_mut().enumerate() {
            rule.source = PermissionRuleSource::ProjectConfiguration {
                fingerprint,
                index: u16::try_from(index).map_err(|_| PermissionChangeError::Capacity)?,
            };
        }
        Ok(Self { fingerprint, rules })
    }

    /// Exact file bytes reviewed by a personal trust decision.
    #[must_use]
    pub const fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// All project rules, including Allow rules that may still need personal activation.
    #[must_use]
    pub fn rules(&self) -> &[PermissionRule] {
        &self.rules
    }
}
