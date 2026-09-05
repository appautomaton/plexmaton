//! Bounded directory names read from one pinned descriptor.

use std::{ffi::OsString, io, os::unix::ffi::OsStringExt as _};

use thiserror::Error;

use crate::{FileCancellation, WorkspaceRoot};

/// Why bounded enumeration of one pinned directory failed.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DirectoryListError {
    #[error("directory enumeration exceeded the {limit}-entry bound")]
    LimitExceeded { limit: usize },
    #[error("directory enumeration I/O failed: {0:?}")]
    Io(io::ErrorKind),
    #[error("directory enumeration was cancelled")]
    Cancelled,
}

impl WorkspaceRoot {
    /// Lists at most `limit` descendant names from the pinned directory descriptor.
    ///
    /// Dot entries are excluded. Names are sorted only after bounded acquisition completes.
    pub fn list_names(
        &self,
        limit: usize,
        cancellation: &FileCancellation,
    ) -> Result<Vec<OsString>, DirectoryListError> {
        if cancellation.is_cancelled() {
            return Err(DirectoryListError::Cancelled);
        }
        let mut directory = rustix::fs::Dir::read_from(self.directory.as_ref())
            .map_err(|error| DirectoryListError::Io(error_kind(error)))?;
        let mut names = Vec::new();
        for entry in &mut directory {
            if cancellation.is_cancelled() {
                return Err(DirectoryListError::Cancelled);
            }
            let entry = entry.map_err(|error| DirectoryListError::Io(error_kind(error)))?;
            let bytes = entry.file_name().to_bytes();
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            if names.len() == limit {
                return Err(DirectoryListError::LimitExceeded { limit });
            }
            names.push(OsString::from_vec(bytes.to_vec()));
        }
        names.sort();
        Ok(names)
    }
}

fn error_kind(error: rustix::io::Errno) -> io::ErrorKind {
    io::Error::from_raw_os_error(error.raw_os_error()).kind()
}
