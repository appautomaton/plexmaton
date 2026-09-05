//! Exact bounded reads for trusted first-party consumers of a pinned root.

use std::io::{self, Read as _};

use thiserror::Error;

use crate::{FileCancellation, PathError, WorkspaceRoot, observation::FileVersion};

/// An exact file prefix read through one pinned workspace root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedFileRead {
    /// Normalized relative path beneath the pinned root.
    pub path: String,
    /// Exact bytes retained, never more than the caller's bound.
    pub bytes: Vec<u8>,
    /// Whether the retained bytes are the complete file.
    pub complete: bool,
}

/// Why a trusted bounded read could not return an authoritative prefix.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum BoundedReadError {
    #[error("the bounded read limit must be positive and representable")]
    InvalidLimit,
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("file I/O failed: {0:?}")]
    Io(io::ErrorKind),
    #[error("the file changed while it was being read")]
    ChangedDuringRead,
    #[error("read was cancelled")]
    Cancelled,
}

impl WorkspaceRoot {
    /// Reads at most `max_bytes` from a no-follow path beneath this pinned root.
    ///
    /// `complete` distinguishes an exact whole file from a bounded prefix. The read compares
    /// descriptor metadata before and after acquisition and returns no bytes after cancellation.
    pub fn read_prefix(
        &self,
        path: &str,
        max_bytes: usize,
        cancellation: &FileCancellation,
    ) -> Result<BoundedFileRead, BoundedReadError> {
        if cancellation.is_cancelled() {
            return Err(BoundedReadError::Cancelled);
        }
        let acquisition_limit = max_bytes
            .checked_add(1)
            .filter(|_| max_bytes > 0)
            .ok_or(BoundedReadError::InvalidLimit)?;
        let (path, mut file) = self.open_file(path)?;
        let before =
            FileVersion::read(&file).map_err(|error| BoundedReadError::Io(error.kind()))?;
        let mut bytes = Vec::with_capacity(acquisition_limit.min(8192));
        file.by_ref()
            .take(u64::try_from(acquisition_limit).unwrap_or(u64::MAX))
            .read_to_end(&mut bytes)
            .map_err(|error| BoundedReadError::Io(error.kind()))?;
        let after = FileVersion::read(&file).map_err(|error| BoundedReadError::Io(error.kind()))?;
        if before != after {
            return Err(BoundedReadError::ChangedDuringRead);
        }
        if cancellation.is_cancelled() {
            return Err(BoundedReadError::Cancelled);
        }
        let complete = bytes.len() <= max_bytes;
        bytes.truncate(max_bytes);
        Ok(BoundedFileRead {
            path,
            bytes,
            complete,
        })
    }
}
