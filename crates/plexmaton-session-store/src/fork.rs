use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use plexmaton_agent::UnixMillis;
use plexmaton_core::SessionId;

use super::codec::{encode_header, encode_line};
use super::{
    JournalFile, JournalRecovery, StoreError, WriterState, lock_writer, secure_open_options,
};

impl JournalFile {
    /// Copies this complete record stream into a new session through a same-directory staging file.
    pub fn fork(
        &self,
        destination: impl AsRef<Path>,
        session_id: SessionId,
        created_at_unix_ms: UnixMillis,
    ) -> Result<Self, StoreError> {
        let destination = destination.as_ref();
        if self.state == WriterState::Poisoned {
            return Err(StoreError::WriterPoisoned);
        }
        if let Some(parent) = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|source| StoreError::io("create fork directory", source))?;
        }
        let (staging_path, mut staging) = reserve_staging(destination)?;
        let mut cleanup = StagingCleanup::new(staging_path.clone());
        lock_writer(&staging)?;
        staging
            .write_all(&encode_header(&session_id, created_at_unix_ms)?)
            .map_err(|source| StoreError::io("write fork header", source))?;
        let mut journal =
            plexmaton_agent::SessionJournal::with_created_at(session_id, created_at_unix_ms);
        for record in self.journal.records() {
            staging
                .write_all(&encode_line(record)?)
                .map_err(|source| StoreError::io("write fork record", source))?;
            journal
                .apply(record.clone())
                .map_err(|reason| StoreError::RejectedRecord { line: 0, reason })?;
        }
        match std::fs::hard_link(&staging_path, destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(StoreError::ForkDestinationExists);
            }
            Err(source) => return Err(StoreError::io("publish fork", source)),
        }
        if std::fs::remove_file(&staging_path).is_ok() {
            cleanup.disarm();
        }
        Ok(Self {
            path: destination.to_path_buf(),
            file: staging,
            journal,
            recovery: JournalRecovery::Clean,
            state: WriterState::Ready,
        })
    }
}

fn reserve_staging(destination: &Path) -> Result<(PathBuf, File), StoreError> {
    for ordinal in 0..32_u8 {
        let candidate =
            destination.with_extension(format!("stage-{}-{ordinal}", std::process::id()));
        let mut options = secure_open_options();
        match options
            .read(true)
            .append(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(StoreError::io("create fork staging file", source)),
        }
    }
    Err(StoreError::ForkStagingExhausted)
}

struct StagingCleanup {
    path: PathBuf,
    armed: bool,
}

impl StagingCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ignored = std::fs::remove_file(&self.path);
        }
    }
}
