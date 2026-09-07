use plexmaton_agent::collaboration::{CollaborationError, CollaborationEvent};
use plexmaton_core::CollaborationItemId;
use thiserror::Error;

/// Failures distinguish rejected input from potentially committed writes.
#[derive(Debug, Error)]
pub enum CollaborationStoreError {
    #[error("collaboration admission refused: {0}")]
    Rejected(#[from] CollaborationError),
    #[error("collaboration file operation failed")]
    Io(#[source] std::io::Error),
    #[error("collaboration framing or file protection failed: {0}")]
    Framing(#[from] crate::StoreError),
    #[error("collaboration format or schema is unsupported")]
    UnsupportedHeader,
    #[error("collaboration file has no header")]
    MissingHeader,
    #[error("collaboration line {line} is malformed")]
    Malformed {
        line: u64,
        #[source]
        source: serde_json::Error,
    },
    #[error("collaboration line {line} violates the ledger: {reason}")]
    InvalidRecord {
        line: u64,
        reason: CollaborationError,
    },
    #[error("collaboration write outcome is unknown; reopen before continuing")]
    WriteUncertain(#[source] std::io::Error),
    #[error("collaboration reduction after write failed; reopen before continuing: {0}")]
    ReductionUncertain(CollaborationError),
    #[error("collaboration writer is poisoned; reopen before continuing")]
    WriterPoisoned,
    #[error("collaboration parent directory must be owner-only and not a symbolic link")]
    InsecureDirectory,
}

/// Exact caller-owned input, retained even when its acceptance is uncertain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaborationAttempt {
    pub id: CollaborationItemId,
    pub event: CollaborationEvent,
}

/// A failed admission returns the attempt for reconciliation; it must not be blindly resubmitted.
#[derive(Debug)]
pub struct CollaborationAppendFailure {
    error: CollaborationStoreError,
    attempt: Box<CollaborationAttempt>,
}

impl CollaborationAppendFailure {
    pub(super) fn new(error: CollaborationStoreError, attempt: CollaborationAttempt) -> Self {
        Self {
            error,
            attempt: Box::new(attempt),
        }
    }

    /// Distinguishes an unwritten rejection from an unknown write or poisoned writer.
    #[must_use]
    pub const fn error(&self) -> &CollaborationStoreError {
        &self.error
    }

    /// Exact requested identity and event, including when bytes may already be on disk.
    #[must_use]
    pub fn attempt(&self) -> &CollaborationAttempt {
        &self.attempt
    }

    /// Returns ownership for reconciliation against a reopened canonical ledger.
    #[must_use]
    pub fn into_parts(self) -> (CollaborationStoreError, CollaborationAttempt) {
        (self.error, *self.attempt)
    }
}
