//! Pure resolution of user-owned configuration paths and credential values.
use super::{ApiKey, ConfigError, ResolvedModel};
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

/// Resolves `PLEXMATON_HOME`, or the user-level `~/.plexmaton` default.
///
/// Project-local `.plexmaton` discovery is deliberately absent: project corpus belongs in
/// `.agents/`, while provider authority remains user-owned (PRV-6).
pub fn resolve_home(
    plexmaton_home: Option<&OsStr>,
    user_home: Option<&Path>,
) -> Result<PathBuf, ConfigError> {
    if let Some(root) = plexmaton_home.filter(|root| !root.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    user_home
        .map(|root| root.join(".plexmaton"))
        .ok_or(ConfigError::HomeUnavailable)
}

/// Validates a value read from the resolved model's named environment variable without globals.
pub fn resolve_api_key(
    model: &ResolvedModel,
    value: Option<OsString>,
) -> Result<ApiKey, ConfigError> {
    let environment = model.api_key_env().to_owned();
    let Some(value) = value else {
        return Err(ConfigError::MissingApiKeyEnvironment(environment));
    };
    let value = value
        .into_string()
        .map_err(|_| ConfigError::InvalidApiKeyValue(environment.clone()))?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(ConfigError::InvalidApiKeyValue(environment));
    }
    Ok(ApiKey(value))
}
