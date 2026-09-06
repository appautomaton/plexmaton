use std::{
    fs::File,
    io::{Read as _, Seek as _, SeekFrom, Write as _},
    time::{Duration, Instant},
};

use plexmaton_agent::PermissionGrant;
use plexmaton_core::{PermissionGrantId, ProjectPermissionRevision};
use rustix::fs::{FlockOperation, OFlags};

use crate::{
    MAX_STORE_BYTES, PermissionStoreError as Error, ProjectPermissionSnapshot,
    codec::{self, Change, Folded, Header, Record},
    paths::{self, INITIALIZED, LOCK, LOG, StorePaths, open_file},
};

/// Short exclusive transaction. Keep this only through mutation or authorization, never execution.
pub struct PermissionTransaction<'a> {
    paths: &'a StorePaths,
    lock: File,
    source: Option<File>,
    folded: Folded,
}

impl<'a> PermissionTransaction<'a> {
    pub(crate) fn open(paths: &'a StorePaths, cancelled: &dyn Fn() -> bool) -> Result<Self, Error> {
        let mut lock = open_file(&paths.directory, LOCK, OFlags::empty())?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            check_cancelled(cancelled)?;
            match rustix::fs::flock(&lock, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => break,
                Err(rustix::io::Errno::WOULDBLOCK) => {
                    if Instant::now() >= deadline {
                        return Err(Error::Busy);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(Error::io("lock project permissions", error)),
            }
        }
        check_cancelled(cancelled)?;
        paths.revalidate()?;
        if !paths::same_file(&lock, &paths.lock)? {
            return Err(Error::IdentityChanged);
        }
        let mut marker = Vec::new();
        (&mut lock)
            .take(INITIALIZED.len() as u64 + 1)
            .read_to_end(&mut marker)
            .map_err(|e| Error::io("read permission lock", e))?;
        let (source, folded) = match open_file(&paths.directory, LOG, OFlags::empty()) {
            Ok(file) => {
                if marker != INITIALIZED {
                    return Err(Error::Corrupt);
                }
                let folded = codec::read(&file, &paths.project)?;
                (Some(file), folded)
            }
            Err(error) if paths::is_missing(&error) && marker.is_empty() => {
                (None, Folded::absent())
            }
            Err(error) if paths::is_missing(&error) => return Err(Error::Corrupt),
            Err(error) => return Err(error),
        };
        check_cancelled(cancelled)?;
        Ok(Self {
            paths,
            lock,
            source,
            folded,
        })
    }

    /// Current fully validated source at this transaction's ordering point.
    #[must_use]
    pub fn snapshot(&self) -> &ProjectPermissionSnapshot {
        &self.folded.snapshot
    }

    /// Consumes the reviewed transaction and returns its locked successor only after acknowledgement.
    /// Failure drops the lock and returns no snapshot; publish memory before dropping the successor.
    pub fn grant(
        mut self,
        expected: &ProjectPermissionRevision,
        grant: PermissionGrant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Error> {
        self.append(expected, Change::Grant { grant }, cancelled)?;
        Ok(self)
    }

    /// Removes exactly one grant, without overriding other explicit policy sources.
    pub fn revoke(
        mut self,
        expected: &ProjectPermissionRevision,
        id: PermissionGrantId,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Error> {
        self.append(expected, Change::Revoke { id }, cancelled)?;
        Ok(self)
    }

    /// Records explicit personal trust (or withdrawal) for the exact project config fingerprint.
    pub fn trust_config(
        mut self,
        expected: &ProjectPermissionRevision,
        fingerprint: Option<[u8; 32]>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Error> {
        self.append(expected, Change::Trust { fingerprint }, cancelled)?;
        Ok(self)
    }

    /// Explicitly clears a healthy store and changes its identity; old revisions cannot match again.
    /// Corrupt sources are refused before a transaction opens and require separate user repair.
    pub fn reset(
        mut self,
        expected: &ProjectPermissionRevision,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Error> {
        self.check(expected, cancelled)?;
        let header = Header::fresh(&self.paths.project);
        self.publish_header(&header, cancelled)?;
        self.folded = header.empty();
        Ok(self)
    }

    fn check(
        &self,
        expected: &ProjectPermissionRevision,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        check_cancelled(cancelled)?;
        if *expected != self.folded.snapshot.revision {
            return Err(Error::StaleRevision);
        }
        self.paths.revalidate()?;
        if let Some(source) = &self.source {
            paths::same_child(&self.paths.directory, LOG, source)?;
        }
        Ok(())
    }

    fn append(
        &mut self,
        expected: &ProjectPermissionRevision,
        change: Change,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        self.append_with(expected, change, cancelled, |file, bytes| {
            file.write_all(bytes).and_then(|()| file.sync_all())
        })
    }

    fn append_with(
        &mut self,
        expected: &ProjectPermissionRevision,
        change: Change,
        cancelled: &dyn Fn() -> bool,
        persist: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<(), Error> {
        self.check(expected, cancelled)?;
        let header = self
            .source
            .is_none()
            .then(|| Header::fresh(&self.paths.project));
        let mut next = header
            .as_ref()
            .map_or_else(|| self.folded.clone(), Header::empty);
        let record = Record {
            sequence: next.sequence().checked_add(1).ok_or(Error::Capacity)?,
            change,
        };
        let bytes = codec::encode(&record)?;
        next.apply(record)?;
        if let Some(header) = header {
            self.publish_header(&header, cancelled)?;
        }
        let file = self.source.as_mut().ok_or(Error::Corrupt)?;
        let size = file
            .metadata()
            .map_err(|e| Error::io("inspect permission size", e))?
            .len();
        if size.saturating_add(bytes.len() as u64) > MAX_STORE_BYTES {
            return Err(Error::Capacity);
        }
        check_cancelled(cancelled)?;
        file.seek(SeekFrom::End(0))
            .map_err(|e| Error::io("seek permission source", e))?;
        // After the first byte, cancellation cannot roll back authority. Acknowledge only after sync.
        persist(file, &bytes).map_err(|_| Error::WriteUncertain)?;
        next.snapshot.can_remember &= size.saturating_add(bytes.len() as u64)
            <= MAX_STORE_BYTES.saturating_sub(crate::MAX_RECORD_BYTES as u64);
        self.folded = next;
        Ok(())
    }

    fn publish_header(
        &mut self,
        header: &Header,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        let bytes = codec::encode(header)?;
        let name = format!(".permissions-{}.tmp", uuid::Uuid::now_v7());
        let mut temporary = Temporary {
            directory: &self.paths.directory,
            name,
            published: false,
        };
        let mut file = open_file(
            &self.paths.directory,
            &temporary.name,
            OFlags::CREATE | OFlags::EXCL,
        )?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| Error::WriteUncertain)?;
        check_cancelled(cancelled)?;
        self.paths.revalidate()?;
        // A durable marker distinguishes a never-written source from a deleted grant/revoke log.
        // Any interrupted initialization fails closed; no former allow can reappear as absence.
        if self.source.is_none() {
            self.lock
                .seek(SeekFrom::Start(0))
                .map_err(|e| Error::io("seek permission lock", e))?;
            self.lock
                .write_all(INITIALIZED)
                .and_then(|()| self.lock.sync_all())
                .map_err(|_| Error::WriteUncertain)?;
        }
        rustix::fs::renameat(
            &self.paths.directory,
            &temporary.name,
            &self.paths.directory,
            LOG,
        )
        .map_err(|_| Error::WriteUncertain)?;
        temporary.published = true;
        self.paths
            .directory
            .sync_all()
            .map_err(|_| Error::WriteUncertain)?;
        self.source = Some(file);
        Ok(())
    }
}

fn check_cancelled(cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

struct Temporary<'a> {
    directory: &'a File,
    name: String,
    published: bool,
}
impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        if !self.published {
            let _ = rustix::fs::unlinkat(
                self.directory,
                self.name.as_str(),
                rustix::fs::AtFlags::empty(),
            );
        }
    }
}

#[cfg(test)]
#[path = "transaction_tests.rs"]
mod tests;
