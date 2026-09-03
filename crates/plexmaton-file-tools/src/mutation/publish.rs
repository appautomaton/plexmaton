use std::{
    ffi::OsString,
    fs::File,
    io::{Seek as _, Write as _},
    sync::atomic::{AtomicU64, Ordering},
};

use rustix::fs::{AtFlags, Mode, OFlags};

use crate::{FileCancellation, observation::FileVersion, path::MutationPath};

use super::{MutationError, read_source};

const TEMP_ATTEMPTS: usize = 16;
pub(super) const WRITE_CHUNK_BYTES: usize = 8192;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Default)]
pub(super) struct PublishHooks<'a> {
    pub(super) before_publish: Option<&'a dyn Fn()>,
    pub(super) before_commit: Option<&'a dyn Fn()>,
    pub(super) after_write_chunk: Option<&'a dyn Fn(usize)>,
    pub(super) fail_write_after: Option<usize>,
}

pub(super) fn publish_replace(
    target: &MutationPath,
    expected_source: &[u8],
    replacement: &[u8],
    expected_version: &FileVersion,
    mode: u16,
    cancellation: &FileCancellation,
    hooks: PublishHooks<'_>,
) -> Result<(), MutationError> {
    let mut staged = StagedFile::create(target)?;
    let result = (|| {
        staged.write(
            replacement,
            cancellation,
            hooks.fail_write_after,
            hooks.after_write_chunk,
        )?;
        rustix::fs::fchmod(staged.file(), Mode::from_bits_retain(mode)).map_err(map_rustix)?;
        staged.file().sync_all().map_err(map_io)?;
        if let Some(hook) = hooks.before_publish {
            hook();
        }
        staged.verify_owned(replacement, cancellation)?;
        let current = target
            .open_existing()
            .map_err(|_| MutationError::ChangedBeforeCommit)?;
        let (current_source, current_version) = read_source(current, cancellation)?;
        if &current_version != expected_version || current_source != expected_source {
            return Err(MutationError::ChangedBeforeCommit);
        }
        if let Some(hook) = hooks.before_commit {
            hook();
        }
        if cancellation.is_cancelled() {
            return Err(MutationError::Cancelled);
        }
        rustix::fs::renameat(
            target.parent(),
            staged.name(),
            target.parent(),
            target.leaf(),
        )
        .map_err(map_rustix)?;
        staged.committed = true;
        let _durability_result = target.parent().sync_all();
        Ok(())
    })();
    staged.cleanup();
    result
}

pub(super) fn publish_create(
    target: &MutationPath,
    contents: &[u8],
    cancellation: &FileCancellation,
    hooks: PublishHooks<'_>,
) -> Result<(), MutationError> {
    let mut staged = StagedFile::create(target)?;
    let result = (|| {
        staged.write(
            contents,
            cancellation,
            hooks.fail_write_after,
            hooks.after_write_chunk,
        )?;
        rustix::fs::fchmod(staged.file(), Mode::from_bits_retain(0o644)).map_err(map_rustix)?;
        staged.file().sync_all().map_err(map_io)?;
        if let Some(hook) = hooks.before_publish {
            hook();
        }
        staged.verify_owned(contents, cancellation)?;
        if let Some(hook) = hooks.before_commit {
            hook();
        }
        if cancellation.is_cancelled() {
            return Err(MutationError::Cancelled);
        }
        match rustix::fs::linkat(
            target.parent(),
            staged.name(),
            target.parent(),
            target.leaf(),
            AtFlags::empty(),
        ) {
            Ok(()) => {}
            Err(error) if error == rustix::io::Errno::EXIST => {
                return Err(MutationError::CreateCollision);
            }
            Err(error) => return Err(map_rustix(error)),
        }
        if rustix::fs::unlinkat(target.parent(), staged.name(), AtFlags::empty()).is_ok() {
            staged.committed = true;
        }
        let _durability_result = target.parent().sync_all();
        Ok(())
    })();
    staged.cleanup();
    result
}

struct StagedFile {
    file: File,
    name: OsString,
    device: u64,
    inode: u64,
    committed: bool,
    parent: File,
}

impl StagedFile {
    fn create(target: &MutationPath) -> Result<Self, MutationError> {
        let parent = target.parent().try_clone().map_err(map_io)?;
        for _attempt in 0..TEMP_ATTEMPTS {
            let counter = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let name = OsString::from(format!(
                ".plexmaton-{}-{counter:016x}.tmp",
                std::process::id()
            ));
            match rustix::fs::openat(
                target.parent(),
                &name,
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::from_bits_retain(0o600),
            ) {
                Ok(descriptor) => {
                    let file = File::from(descriptor);
                    let metadata = match rustix::fs::fstat(&file) {
                        Ok(metadata) => metadata,
                        Err(error) => {
                            let _cleanup =
                                rustix::fs::unlinkat(target.parent(), &name, AtFlags::empty());
                            return Err(map_rustix(error));
                        }
                    };
                    return Ok(Self {
                        file,
                        name,
                        device: metadata.st_dev as u64,
                        inode: metadata.st_ino,
                        committed: false,
                        parent,
                    });
                }
                Err(error) if error == rustix::io::Errno::EXIST => {}
                Err(error) => return Err(map_rustix(error)),
            }
        }
        Err(MutationError::Io(std::io::ErrorKind::AlreadyExists))
    }

    fn file(&self) -> &File {
        &self.file
    }

    fn name(&self) -> &OsString {
        &self.name
    }

    fn write(
        &mut self,
        contents: &[u8],
        cancellation: &FileCancellation,
        fail_after: Option<usize>,
        after_chunk: Option<&dyn Fn(usize)>,
    ) -> Result<(), MutationError> {
        let mut written = 0_usize;
        while written < contents.len() {
            if cancellation.is_cancelled() {
                return Err(MutationError::Cancelled);
            }
            if fail_after.is_some_and(|limit| written >= limit) {
                return Err(MutationError::Io(std::io::ErrorKind::WriteZero));
            }
            let mut end = written
                .saturating_add(WRITE_CHUNK_BYTES)
                .min(contents.len());
            if let Some(limit) = fail_after {
                end = end.min(limit);
            }
            if end == written {
                return Err(MutationError::Io(std::io::ErrorKind::WriteZero));
            }
            self.file
                .write_all(&contents[written..end])
                .map_err(map_io)?;
            written = end;
            if let Some(hook) = after_chunk {
                hook(written);
            }
        }
        Ok(())
    }

    fn verify_owned(
        &self,
        expected: &[u8],
        cancellation: &FileCancellation,
    ) -> Result<(), MutationError> {
        let mut reader = self.file.try_clone().map_err(map_io)?;
        reader.seek(std::io::SeekFrom::Start(0)).map_err(map_io)?;
        let (contents, descriptor_version) =
            read_source(reader, cancellation).map_err(map_staging_read)?;
        if contents != expected {
            return Err(MutationError::StagingChanged);
        }
        let reopened = rustix::fs::openat(
            &self.parent,
            &self.name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|_| MutationError::StagingChanged)?;
        let path_version =
            FileVersion::read(&reopened).map_err(|_| MutationError::StagingChanged)?;
        if path_version != descriptor_version {
            return Err(MutationError::StagingChanged);
        }
        Ok(())
    }

    fn cleanup(&self) {
        if self.committed {
            return;
        }
        let Ok(metadata) = rustix::fs::statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW)
        else {
            return;
        };
        if metadata.st_dev as u64 == self.device && metadata.st_ino == self.inode {
            let _cleanup = rustix::fs::unlinkat(&self.parent, &self.name, AtFlags::empty());
        }
    }
}

fn map_rustix(error: rustix::io::Errno) -> MutationError {
    map_io(std::io::Error::from(error))
}

fn map_io(error: std::io::Error) -> MutationError {
    MutationError::Io(error.kind())
}

fn map_staging_read(error: MutationError) -> MutationError {
    match error {
        MutationError::Cancelled => MutationError::Cancelled,
        _ => MutationError::StagingChanged,
    }
}
