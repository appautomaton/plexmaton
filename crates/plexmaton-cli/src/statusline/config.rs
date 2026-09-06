use std::time::Duration;

use anyhow::bail;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusLineConfig {
    pub command: String,
    #[serde(default = "default_rows")]
    pub max_rows: u16,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    /// Optional wall-clock/Git refresh. Semantic changes always schedule an update.
    pub refresh_ms: Option<u64>,
}

const fn default_rows() -> u16 {
    6
}
const fn default_timeout() -> u64 {
    1000
}

impl StatusLineConfig {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        if self.command.trim().is_empty()
            || self.command.len() > 4096
            || self.command.contains('\0')
        {
            bail!("status_line.command must contain 1–4096 bytes and no NUL");
        }
        if !(1..=64).contains(&self.max_rows) || !(10..=5000).contains(&self.timeout_ms) {
            bail!("status_line requires max_rows in 1–64 and timeout_ms in 10–5000");
        }
        if self
            .refresh_ms
            .is_some_and(|ms| !(1000..=3_600_000).contains(&ms))
        {
            bail!("status_line.refresh_ms must be in 1000–3600000, or omitted");
        }
        Ok(())
    }
}
