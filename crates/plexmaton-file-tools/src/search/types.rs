//! Bounded search request, result, error, and cancellation vocabulary.

use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use thiserror::Error;

use crate::{PathError, path::normalize_search_path};

pub const DEFAULT_SEARCH_MATCHES: u16 = 100;
pub const MAX_SEARCH_MATCHES: u16 = 500;
pub(crate) const MAX_PATTERN_CHARACTERS: usize = 8 * 1024;
pub const MAX_PATTERN_BYTES: usize = 8 * 1024;
pub(crate) const MAX_GLOB_CHARACTERS: usize = 4096;
const MAX_GLOB_BYTES: usize = 4096;
pub const MAX_PREVIEW_BYTES: usize = 2 * 1024;
pub const MAX_RG_RECORD_BYTES: usize = 64 * 1024;
pub const MAX_RG_STDOUT_BYTES: usize = 1024 * 1024;
pub const MAX_RG_STDERR_BYTES: usize = 64 * 1024;
pub const MAX_SEARCH_FILES: usize = 512;
pub const MAX_SEARCH_PATH_BYTES: usize = 4096;
pub const MAX_SEARCH_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// Raw path and preview bytes retained before JSON escaping.
pub const MAX_SEARCH_RESULT_BYTES: usize = 128 * 1024;

/// Validated model request for one bounded search.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchRequest {
    pub(super) pattern: String,
    pub(super) path: String,
    pub(super) glob: Option<String>,
    pub(super) limit: u16,
}

impl SearchRequest {
    pub fn new(
        pattern: String,
        path: Option<String>,
        glob: Option<String>,
        limit: Option<u16>,
    ) -> Result<Self, SearchError> {
        let path = path.unwrap_or_else(|| ".".to_owned());
        let limit = limit.unwrap_or(DEFAULT_SEARCH_MATCHES);
        let path = normalize_search_path(&path)?;
        if pattern.is_empty()
            || pattern.len() > MAX_PATTERN_BYTES
            || glob
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > MAX_GLOB_BYTES)
            || limit == 0
            || limit > MAX_SEARCH_MATCHES
        {
            return Err(SearchError::InvalidArguments);
        }
        Ok(Self {
            pattern,
            path,
            glob,
            limit,
        })
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::{SearchError, SearchRequest};

    #[test]
    fn search_text_enforces_decoded_utf8_byte_bounds() {
        assert!(SearchRequest::new("🦀".repeat(2048), None, None, None).is_ok());
        assert_eq!(
            SearchRequest::new("🦀".repeat(2049), None, None, None),
            Err(SearchError::InvalidArguments)
        );
        assert!(
            SearchRequest::new("pattern".to_owned(), None, Some("🦀".repeat(1024)), None).is_ok()
        );
        assert_eq!(
            SearchRequest::new("pattern".to_owned(), None, Some("🦀".repeat(1025)), None),
            Err(SearchError::InvalidArguments)
        );
    }
}

/// Explicit owner-controlled cancellation shared by blocking file-tool boundaries (WFS-6).
#[derive(Clone, Default)]
pub struct FileCancellation(Arc<AtomicBool>);

impl FileCancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Why a successful bounded search stopped acquiring matches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchCompletion {
    Complete,
    FileLimit,
    FileByteLimit,
    MatchLimit,
    TransportByteLimit,
    RetainedByteLimit,
}

/// One exact location with a bounded UTF-8 line preview.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchMatch {
    pub path: String,
    pub line: u64,
    pub preview: String,
    pub preview_truncated: bool,
}

/// Retained matches and observable acquisition facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub completion: SearchCompletion,
    pub matches_seen: u64,
    pub transport_bytes: usize,
    pub stderr: String,
    pub stderr_omitted_bytes: usize,
}

/// Typed search refusal, process failure, or cancellation.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SearchError {
    #[error("search arguments are outside their hard bounds")]
    InvalidArguments,
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("ripgrep could not be spawned: {0:?}")]
    Spawn(io::ErrorKind),
    #[error("search executables must be trusted absolute paths")]
    UntrustedExecutable,
    #[error("ripgrep I/O failed: {0:?}")]
    Io(io::ErrorKind),
    #[error("one ripgrep JSON record exceeded {limit} bytes")]
    RecordTooLarge { limit: usize },
    #[error("one discovered search path exceeded {limit} bytes")]
    CandidateTooLong { limit: usize },
    #[error("ripgrep returned malformed or binary JSON output")]
    InvalidProtocol,
    #[error("a file changed while its search result was being produced")]
    ChangedDuringSearch,
    #[error("ripgrep exited unsuccessfully with {code:?}: {stderr}")]
    ProcessFailed {
        code: Option<i32>,
        stderr: String,
        omitted_bytes: usize,
    },
    #[error("search was cancelled")]
    Cancelled,
    #[error("search exceeded its deadline")]
    TimedOut,
    #[error("a ripgrep reader task terminated unexpectedly")]
    ReaderFailed,
}
