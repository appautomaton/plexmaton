//! Personal project permission storage. Blocking operations belong in an owned worker (PGR-1–PGR-5).

use std::{io, path::Path, sync::Arc};

use plexmaton_agent::PermissionGrant;
use plexmaton_core::ProjectPermissionRevision;
use thiserror::Error;

mod codec;
mod identity;
#[cfg(unix)]
mod paths;
#[cfg(unix)]
mod transaction;

pub use identity::ProjectIdentity;
#[cfg(unix)]
pub use transaction::PermissionTransaction;

/// Hard bounds apply to the whole source, including revoked grant identities.
pub const MAX_STORE_BYTES: u64 = 16 * 1024 * 1024;
/// No record may allocate more than this encoded byte count.
pub const MAX_RECORD_BYTES: usize = 192 * 1024;
/// Mutation retention is bounded; reset is an explicit operation, never automatic salvage.
pub const MAX_STORE_RECORDS: u64 = 4096;

/// Failure of a personal permission source. Absence is a successful empty snapshot.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PermissionStoreError {
    #[error("project permissions are not supported on this platform")]
    UnsupportedPlatform,
    #[error("the project or personal permission path changed")]
    IdentityChanged,
    #[error("the permission path is not an owner-controlled regular file or directory")]
    UnsafePath,
    #[error("the project permission source is malformed or incomplete")]
    Corrupt,
    #[error("the project permission format is unsupported")]
    UnsupportedFormat,
    #[error("project permissions changed; review the current permissions")]
    StaleRevision,
    #[error("the project permission source reached its bounded capacity")]
    Capacity,
    #[error("the project grant does not exist")]
    NotFound,
    #[error("project permission work was cancelled")]
    Cancelled,
    #[error("another process holds the project permission lock")]
    Busy,
    #[error("the permission write could not be acknowledged; its result is unknown")]
    WriteUncertain,
    #[error("cannot {operation}: {kind:?}")]
    Io {
        operation: &'static str,
        kind: io::ErrorKind,
    },
}

impl PermissionStoreError {
    pub(crate) fn io(operation: &'static str, error: impl Into<io::Error>) -> Self {
        Self::Io {
            operation,
            kind: error.into().kind(),
        }
    }
}

/// Immutable result of a fully validated, locked read; dispatch still refreshes this source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectPermissionSnapshot {
    pub(crate) can_remember: bool,
    pub revision: ProjectPermissionRevision,
    pub grants: Vec<PermissionGrant>,
    /// Explicit personal trust in one exact bounded project configuration fingerprint.
    pub trusted_config: Option<[u8; 32]>,
}

impl ProjectPermissionSnapshot {
    /// Whether a maximum-sized grant fits current count, revision and whole-source bounds.
    #[must_use]
    pub const fn can_remember(&self) -> bool {
        self.can_remember
    }
}

/// Descriptor-pinned personal store. Clones share identity, never independently mutable grants.
#[derive(Clone)]
pub struct ProjectPermissionStore {
    #[cfg(unix)]
    paths: Arc<paths::StorePaths>,
}

impl ProjectPermissionStore {
    /// Opens private directories and a stable lock, but creates no permission JSONL until mutation.
    /// `project` is the discovered physical project root, which may differ from execution cwd.
    pub fn open(home: &Path, project: &Path) -> Result<Self, PermissionStoreError> {
        #[cfg(unix)]
        {
            Ok(Self {
                paths: Arc::new(paths::StorePaths::open(home, project)?),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (home, project);
            Err(PermissionStoreError::UnsupportedPlatform)
        }
    }

    /// Current physical project identity used in the store header and namespace.
    #[cfg(unix)]
    #[must_use]
    pub fn project(&self) -> &ProjectIdentity {
        &self.paths.project
    }

    /// Takes a bounded, cancellation-aware exclusive transaction and reloads the entire source.
    /// Dispatch and revocation use this same ordering point. Drop releases the OS lock.
    #[cfg(unix)]
    pub fn transaction(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PermissionTransaction<'_>, PermissionStoreError> {
        PermissionTransaction::open(&self.paths, cancelled)
    }

    /// Loads the current source under the mutation lock; no valid-prefix recovery exists.
    #[cfg(unix)]
    pub fn read(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProjectPermissionSnapshot, PermissionStoreError> {
        Ok(self.transaction(cancelled)?.snapshot().clone())
    }
}

#[cfg(test)]
mod tests;
