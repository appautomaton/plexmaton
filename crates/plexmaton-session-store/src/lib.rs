//! Per-session JSONL persistence for the canonical journal.
//!
//! The store owns file format, writer exclusion and tail recovery. It does not interpret session
//! facts or perform replayed effects; those remain in `plexmaton-agent` (JRN-4, JRN-5).

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

use plexmaton_agent::{ConversationJournal, JournalRecord, UnixMillis};
use plexmaton_core::ConversationId;

mod automatic;
#[cfg(test)]
mod automatic_tests;
mod codec;
pub mod collaboration;
pub use automatic::AutomaticJournal;
mod error;
mod fork;
mod load;
mod paths;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tree_tests;

pub use codec::{MAX_JOURNAL_LINE_BYTES, SCHEMA_EPOCH};
pub use error::{AppendFailure, StoreError};
pub use load::JournalRecovery;
pub use paths::ConversationDirectory;

use codec::{encode_header, encode_line};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WriterState {
    Ready,
    Poisoned,
}

/// The one exclusive writer and in-memory reduction for a session JSONL file.
pub struct JournalFile {
    path: PathBuf,
    file: File,
    journal: ConversationJournal,
    recovery: JournalRecovery,
    state: WriterState,
}

impl JournalFile {
    /// Creates a new locked session file and writes its typed header.
    pub fn create(
        path: impl AsRef<Path>,
        session_id: ConversationId,
        created_at_unix_ms: UnixMillis,
    ) -> Result<Self, StoreError> {
        let path = path.as_ref();
        let header = encode_header(&session_id, created_at_unix_ms)?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|source| StoreError::io("create parent directory", source))?;
        }
        let mut options = secure_open_options();
        let mut file = options
            .read(true)
            .append(true)
            .create_new(true)
            .open(path)
            .map_err(|source| StoreError::io("create", source))?;
        if let Err(error) = lock_writer(&file) {
            drop(file);
            let _cleanup = std::fs::remove_file(path);
            return Err(error);
        }
        if let Err(source) = file.write_all(&header) {
            drop(file);
            let _cleanup = std::fs::remove_file(path);
            return Err(StoreError::io("write header", source));
        }
        Ok(Self {
            path: path.to_path_buf(),
            file,
            journal: ConversationJournal::with_created_at(session_id, created_at_unix_ms),
            recovery: JournalRecovery::Clean,
            state: WriterState::Ready,
        })
    }

    /// Opens, locks, validates and if necessary repairs one final syntactic tail.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        reject_symlink(path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(path)
            .map_err(|source| StoreError::io("open", source))?;
        ensure_owner_only(&file)?;
        lock_writer(&file)?;
        let loaded = load::load(&mut file, path)?;
        file.seek(SeekFrom::End(0))
            .map_err(|source| StoreError::io("seek end", source))?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            journal: loaded.journal,
            recovery: loaded.recovery,
            state: WriterState::Ready,
        })
    }

    /// Canonical in-memory reduction of every complete record currently in the file.
    #[must_use]
    pub const fn journal(&self) -> &ConversationJournal {
        &self.journal
    }

    /// Recovery performed while opening this file.
    #[must_use]
    pub const fn recovery(&self) -> &JournalRecovery {
        &self.recovery
    }

    /// Filesystem path held by this writer.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one complete record before applying that same value to memory (JRN-4).
    pub fn append(&mut self, record: JournalRecord) -> Result<(), AppendFailure> {
        self.append_with(record, |file, encoded| file.write_all(encoded))
    }

    fn append_with(
        &mut self,
        record: JournalRecord,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<(), AppendFailure> {
        if self.state == WriterState::Poisoned {
            return Err(AppendFailure::new(StoreError::WriterPoisoned, record));
        }
        if let Err(reason) = self.journal.validate_record(&record) {
            return Err(AppendFailure::new(
                StoreError::RejectedRecord { line: 0, reason },
                record,
            ));
        }
        let encoded = match encode_line(&record) {
            Ok(encoded) => encoded,
            Err(error) => return Err(AppendFailure::new(error, record)),
        };
        if let Err(source) = write(&mut self.file, &encoded) {
            self.state = WriterState::Poisoned;
            return Err(AppendFailure::new(StoreError::io("append", source), record));
        }
        if let Err(reason) = self.journal.apply(record.clone()) {
            self.state = WriterState::Poisoned;
            return Err(AppendFailure::new(
                StoreError::RejectedRecord { line: 0, reason },
                record,
            ));
        }
        Ok(())
    }
}

impl Drop for JournalFile {
    fn drop(&mut self) {
        // JRN-4: a concurrent fork can briefly inherit this open-file description before exec.
        // Closing only our descriptor would let that child prolong the departed writer's flock.
        // No file handle is exposed; release writer authority before the owned descriptor closes.
        let _release = self.file.unlock();
    }
}

fn reject_symlink(path: &Path) -> Result<(), StoreError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|source| StoreError::io("inspect path", source))?;
    if metadata.file_type().is_symlink() {
        return Err(StoreError::SymlinkPath);
    }
    Ok(())
}

fn lock_writer(file: &File) -> Result<(), StoreError> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(StoreError::WriterLocked),
        Err(TryLockError::Error(source)) => Err(StoreError::io("lock", source)),
    }
}

pub(crate) fn secure_open_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    options.mode(0o600);
    options
}

fn ensure_owner_only(file: &File) -> Result<(), StoreError> {
    #[cfg(unix)]
    {
        let mode = file
            .metadata()
            .map_err(|source| StoreError::io("inspect permissions", source))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(StoreError::InsecurePermissions(mode));
        }
    }
    Ok(())
}
