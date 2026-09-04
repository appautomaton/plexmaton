use std::io;

use plexmaton_agent::{JournalError, JournalRecord};
use thiserror::Error;

/// Why a session journal file could not be created, loaded, or advanced.
#[derive(Debug, Error)]
pub enum StoreError {
    /// A user-facing session name cannot be represented as one safe file in the sessions root.
    #[error("session id is not a portable file name")]
    InvalidSessionFileName,
    /// A session path was a symbolic link rather than owned storage.
    #[error("session journal path cannot be a symbolic link")]
    SymlinkPath,
    /// A filesystem operation failed.
    #[error("session journal {operation} failed")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    /// Another writer owns the session file.
    #[error("session journal already has a writer")]
    WriterLocked,
    /// A journal-derived file is readable or writable by another account.
    #[error("session journal permissions {0:o} are not owner-only")]
    InsecurePermissions(u32),
    /// The sessions root is readable or writable by another account.
    #[error("sessions directory permissions {0:o} are not owner-only")]
    InsecureDirectoryPermissions(u32),
    /// Every bounded candidate for one automatically named session already existed.
    #[error("could not reserve an automatic session name")]
    AutomaticSessionNameExhausted,
    /// The file contained no format header.
    #[error("session journal has no header")]
    MissingHeader,
    /// The first line was not the session-journal header.
    #[error("session journal header format is unsupported")]
    UnsupportedHeader,
    /// The header selected a schema epoch this binary does not implement.
    #[error("session journal schema epoch {0} is unsupported")]
    UnsupportedSchema(String),
    /// A JSON line exceeded the hard retained/encoded bound.
    #[error("session journal line {line} exceeds {limit} bytes")]
    LineTooLarge { line: u64, limit: usize },
    /// A line did not decode as its declared typed value.
    #[error("session journal line {line} is malformed")]
    MalformedLine {
        line: u64,
        #[source]
        source: serde_json::Error,
    },
    /// A decoded record violated ordering, ancestry, or revision rules.
    #[error("session journal line {line} was rejected: {reason:?}")]
    RejectedRecord { line: u64, reason: JournalError },
    /// A write failed after bytes may have reached the file; reopen must recover the tail.
    #[error("session journal writer is poisoned; reopen it before appending")]
    WriterPoisoned,
    /// The complete file prepared for a fork would overwrite an existing session.
    #[error("fork destination already exists")]
    ForkDestinationExists,
    /// No collision-free staging sibling could be reserved.
    #[error("could not reserve a staging file for the session fork")]
    ForkStagingExhausted,
    /// No collision-free sibling could retain a malformed final tail.
    #[error("could not reserve a file for the malformed journal tail")]
    TailStagingExhausted,
}

impl StoreError {
    pub(crate) fn io(operation: &'static str, source: io::Error) -> Self {
        Self::Io { operation, source }
    }
}

/// An append failure that returns the exact record ownership to its caller.
#[derive(Debug)]
pub struct AppendFailure {
    error: StoreError,
    record: Box<JournalRecord>,
}

impl AppendFailure {
    pub(crate) fn new(error: StoreError, record: JournalRecord) -> Self {
        Self {
            error,
            record: Box::new(record),
        }
    }

    /// Typed storage failure.
    #[must_use]
    pub const fn error(&self) -> &StoreError {
        &self.error
    }

    /// Record that did not enter the old in-memory journal; its bytes may still be on disk.
    #[must_use]
    pub fn record(&self) -> &JournalRecord {
        self.record.as_ref()
    }

    /// Returns both parts for reconciliation after reopen; callers must not blindly retry.
    #[must_use]
    pub fn into_parts(self) -> (StoreError, JournalRecord) {
        (self.error, *self.record)
    }
}
