//! Exclusive JSONL admission for a canonical collaboration ledger (COL-4/COL-5).
//!
//! This blocking component must be owned by a runtime writer worker, never the TUI event loop.
//! It acknowledges accepted facts only; it does not schedule recipients or claim consumption.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use plexmaton_agent::collaboration::{
    CollaborationEvent, CollaborationLedger, CollaborationLimits, ItemReceipt, Preparation,
};
use plexmaton_core::{CollaborationId, CollaborationItemId};

use crate::{
    JournalRecovery, WriterState, ensure_owner_only, lock_writer, reject_symlink,
    secure_open_options,
};

mod codec;
mod error;
#[cfg(test)]
mod tests;

pub use error::{CollaborationAppendFailure, CollaborationAttempt, CollaborationStoreError};

/// One locked collaboration file and its reduction; no independent mailbox or delegation file.
pub struct CollaborationFile {
    path: PathBuf,
    file: File,
    ledger: CollaborationLedger,
    recovery: JournalRecovery,
    state: WriterState,
}

impl CollaborationFile {
    /// Creates an owner-only file under an owner-only parent, refusing an existing destination.
    pub fn create(
        path: impl AsRef<Path>,
        id: CollaborationId,
        limits: CollaborationLimits,
    ) -> Result<Self, CollaborationStoreError> {
        let path = path.as_ref();
        let ledger = CollaborationLedger::new(id, limits)?;
        let header = codec::header(&ledger)?;
        codec::ensure_parent(path)?;
        let mut file = secure_open_options()
            .read(true)
            .append(true)
            .create_new(true)
            .open(path)
            .map_err(CollaborationStoreError::Io)?;
        if let Err(error) = lock_writer(&file) {
            drop(file);
            let _cleanup = std::fs::remove_file(path);
            return Err(error.into());
        }
        if let Err(error) = file.write_all(&header) {
            drop(file);
            let _cleanup = std::fs::remove_file(path);
            return Err(CollaborationStoreError::Io(error));
        }
        Ok(Self {
            path: path.to_path_buf(),
            file,
            ledger,
            recovery: JournalRecovery::Clean,
            state: WriterState::Ready,
        })
    }

    /// Locks and validates before repairing a syntactically incomplete final tail.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CollaborationStoreError> {
        let path = path.as_ref();
        codec::check_parent(path)?;
        reject_symlink(path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(path)
            .map_err(CollaborationStoreError::Io)?;
        ensure_owner_only(&file)?;
        lock_writer(&file)?;
        let (ledger, recovery) = codec::load(&mut file, path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            ledger,
            recovery,
            state: WriterState::Ready,
        })
    }

    /// Only acknowledged records appear here, even after an uncertain write.
    #[must_use]
    pub const fn ledger(&self) -> &CollaborationLedger {
        &self.ledger
    }

    /// Physical tail repair performed by this open; callers can surface the isolated evidence.
    #[must_use]
    pub const fn recovery(&self) -> &JournalRecovery {
        &self.recovery
    }

    /// Exact file whose writer lock is held until this owner drops.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the canonical original receipt on exact retry, including after reopen.
    pub fn admit(
        &mut self,
        id: CollaborationItemId,
        event: CollaborationEvent,
    ) -> Result<ItemReceipt, CollaborationAppendFailure> {
        self.admit_with(CollaborationAttempt { id, event }, |file, bytes| {
            file.write_all(bytes)
        })
    }

    fn admit_with(
        &mut self,
        attempt: CollaborationAttempt,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<ItemReceipt, CollaborationAppendFailure> {
        let outcome = self.admit_inner(&attempt, write);
        outcome.map_err(|error| CollaborationAppendFailure::new(error, attempt))
    }

    fn admit_inner(
        &mut self,
        attempt: &CollaborationAttempt,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<ItemReceipt, CollaborationStoreError> {
        // Even a previously accepted retry cannot acknowledge through an uncertain writer.
        if self.state == WriterState::Poisoned {
            return Err(CollaborationStoreError::WriterPoisoned);
        }
        let record = match self
            .ledger
            .prepare(attempt.id.clone(), attempt.event.clone())?
        {
            Preparation::Existing(receipt) => return Ok(receipt),
            Preparation::Append(record) => record,
        };
        let bytes = crate::codec::encode_line(&record)?;
        if let Err(error) = write(&mut self.file, &bytes) {
            self.state = WriterState::Poisoned;
            return Err(CollaborationStoreError::WriteUncertain(error));
        }
        self.ledger.apply(*record).map_err(|error| {
            self.state = WriterState::Poisoned;
            CollaborationStoreError::ReductionUncertain(error)
        })
    }
}

impl Drop for CollaborationFile {
    fn drop(&mut self) {
        // COL-5: an inherited duplicate descriptor must not prolong this owner's authority.
        let _release = self.file.unlock();
    }
}
