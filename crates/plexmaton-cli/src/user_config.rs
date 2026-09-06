//! User configuration composition. One TOML parse feeds the typed owners of each root table.
use anyhow::Context as _;
use plexmaton_provider::ModelRegistry;
use sha2::{Digest as _, Sha256};

use crate::{permission_config::PermissionDeclarations, statusline::StatusLineConfig};

#[derive(Debug)]
pub(crate) struct UserConfig {
    pub models: ModelRegistry,
    pub status_line: Option<StatusLineConfig>,
    pub permissions: PermissionDeclarations,
    pub fingerprint: [u8; 32],
}

pub(crate) fn parse(source: &str) -> anyhow::Result<UserConfig> {
    // TOML errors may quote credential-bearing source lines; only typed section names are exposed.
    let mut table: toml::Table =
        toml::from_str(source).map_err(|_| anyhow::anyhow!("invalid user TOML configuration"))?;
    let status_line = table
        .remove("status_line")
        .map(|value| {
            let config: StatusLineConfig = value
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid status_line configuration"))?;
            config.validate()?;
            Ok::<_, anyhow::Error>(config)
        })
        .transpose()?;
    let permissions = table
        .remove("permissions")
        .map(|value| {
            value
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid permissions configuration"))
        })
        .transpose()?
        .unwrap_or_default();
    Ok(UserConfig {
        models: ModelRegistry::from_table(table).context("parse model configuration")?,
        status_line,
        permissions,
        fingerprint: Sha256::digest(source.as_bytes()).into(),
    })
}
