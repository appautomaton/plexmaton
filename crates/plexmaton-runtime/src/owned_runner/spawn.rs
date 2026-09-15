//! Spawn refusal that preserves the live runtime owner.

use super::*;

/// Spawn refusal that returns the live runtime without dropping its journal owner.
pub struct OwnedRunnerSpawnError {
    pub(super) source: OwnedRunnerError,
    pub(super) runtime: Box<LiveRuntime>,
}

impl OwnedRunnerSpawnError {
    /// Returns the runtime for explicit shutdown or another valid owner.
    #[must_use]
    pub fn into_runtime(self) -> LiveRuntime {
        *self.runtime
    }
}

impl fmt::Debug for OwnedRunnerSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnedRunnerSpawnError")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for OwnedRunnerSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for OwnedRunnerSpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
