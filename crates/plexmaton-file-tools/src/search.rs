//! Direct-argv ripgrep with bounded acquisition, retention, cancellation, and joining.

use std::{
    ffi::OsString,
    path::PathBuf,
    process::{Child, ExitStatus},
    time::{Duration, Instant},
};

use crate::{WorkspaceRoot, path::SearchTarget};

mod discover;
mod file;
mod pump;
mod snapshot;
mod types;

use discover::discover;
use file::FileSearchLimits;
use pump::{BoundedBytes, PumpEvent, collect_bounded, pump_stdout};
pub use types::*;

const SEARCH_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_SEARCH_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CHANNEL_CAPACITY: usize = 32;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Concrete ripgrep program and deadline policy.
#[derive(Clone, Debug)]
pub struct SearchRunner {
    executable: PathBuf,
    directory_driver: DirectoryDriver,
    timeout: Duration,
}

#[derive(Clone, Debug)]
struct DirectoryDriver {
    program: PathBuf,
    prefix: Vec<OsString>,
}

impl SearchRunner {
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>, directory_driver: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            directory_driver: DirectoryDriver {
                program: directory_driver.into(),
                prefix: Vec::new(),
            },
            timeout: SEARCH_TIMEOUT,
        }
    }

    /// Installs fixed trusted arguments which precede ripgrep's executable in driver invocations.
    #[must_use]
    pub fn with_directory_driver_prefix(mut self, prefix: Vec<OsString>) -> Self {
        self.directory_driver.prefix = prefix;
        self
    }

    /// Overrides the trusted executor deadline; model arguments cannot change this policy.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.min(MAX_SEARCH_TIMEOUT);
        self
    }

    pub fn search(
        &self,
        root: &WorkspaceRoot,
        request: &SearchRequest,
        cancellation: &FileCancellation,
    ) -> Result<SearchResult, SearchError> {
        if cancellation.is_cancelled() {
            return Err(SearchError::Cancelled);
        }
        if !self.executable.is_absolute() {
            return Err(SearchError::UntrustedExecutable);
        }
        let deadline = Instant::now() + self.timeout;
        let mut target = root.search_target(&request.path)?;
        if target.file.is_some() && request.glob.is_some() {
            return Err(SearchError::InvalidArguments);
        }
        if target.file.is_none() && !self.directory_driver.program.is_absolute() {
            return Err(SearchError::UntrustedExecutable);
        }
        let validation = self.search_input(
            Vec::new(),
            ".",
            request,
            cancellation,
            deadline,
            FileSearchLimits {
                matches: 1,
                retained_bytes: MAX_SEARCH_RESULT_BYTES,
                transport_bytes: MAX_RG_STDOUT_BYTES,
                validation_only: true,
            },
        )?;
        if validation.completion != SearchCompletion::Complete {
            return Ok(SearchResult {
                matches: Vec::new(),
                matches_seen: 0,
                ..validation
            });
        }
        if let Some(file) = target.file.take() {
            let mut result = self.search_file(
                file,
                &target.display,
                request,
                cancellation,
                deadline,
                FileSearchLimits {
                    matches: request.limit,
                    retained_bytes: MAX_SEARCH_RESULT_BYTES,
                    transport_bytes: MAX_RG_STDOUT_BYTES.saturating_sub(validation.transport_bytes),
                    validation_only: false,
                },
            )?;
            result.transport_bytes = result
                .transport_bytes
                .saturating_add(validation.transport_bytes);
            return Ok(result);
        }
        self.search_directory(
            target,
            request,
            cancellation,
            deadline,
            validation.transport_bytes,
        )
    }

    fn search_directory(
        &self,
        target: SearchTarget,
        request: &SearchRequest,
        cancellation: &FileCancellation,
        deadline: Instant,
        initial_transport_bytes: usize,
    ) -> Result<SearchResult, SearchError> {
        let discovery = discover(
            &self.executable,
            &self.directory_driver,
            target
                .directory
                .try_clone()
                .map_err(|error| SearchError::Io(error.kind()))?,
            request.glob.as_deref(),
            cancellation,
            deadline,
            MAX_RG_STDOUT_BYTES.saturating_sub(initial_transport_bytes),
        )?;
        if discovery.completion == Some(SearchCompletion::TransportByteLimit) {
            return Ok(SearchResult {
                matches: Vec::new(),
                completion: SearchCompletion::TransportByteLimit,
                matches_seen: 0,
                transport_bytes: initial_transport_bytes.saturating_add(discovery.transport_bytes),
                stderr: String::new(),
                stderr_omitted_bytes: 0,
            });
        }
        let pending_completion = discovery.completion;
        let mut combined = SearchResult {
            matches: Vec::new(),
            completion: SearchCompletion::Complete,
            matches_seen: 0,
            transport_bytes: initial_transport_bytes.saturating_add(discovery.transport_bytes),
            stderr: String::new(),
            stderr_omitted_bytes: 0,
        };
        for candidate in &discovery.candidates {
            let (logical_path, file) = target.open_child_file(candidate)?;
            let retained = retained_match_bytes(&combined.matches);
            let result = self.search_file(
                file,
                &logical_path,
                request,
                cancellation,
                deadline,
                FileSearchLimits {
                    matches: request
                        .limit
                        .saturating_sub(u16::try_from(combined.matches.len()).unwrap_or(u16::MAX)),
                    retained_bytes: MAX_SEARCH_RESULT_BYTES.saturating_sub(retained),
                    transport_bytes: MAX_RG_STDOUT_BYTES.saturating_sub(combined.transport_bytes),
                    validation_only: false,
                },
            )?;
            combined.matches_seen = combined.matches_seen.saturating_add(result.matches_seen);
            combined.transport_bytes = combined
                .transport_bytes
                .saturating_add(result.transport_bytes);
            combined.matches.extend(result.matches);
            if !result.stderr.is_empty() {
                combined.stderr = result.stderr;
                combined.stderr_omitted_bytes = result.stderr_omitted_bytes;
            }
            if result.completion != SearchCompletion::Complete {
                combined.completion = result.completion;
                return Ok(combined);
            }
        }
        combined.completion = pending_completion.unwrap_or(SearchCompletion::Complete);
        Ok(combined)
    }
}

fn retained_match_bytes(matches: &[SearchMatch]) -> usize {
    matches.iter().fold(0_usize, |total, found| {
        total
            .saturating_add(found.path.len())
            .saturating_add(found.preview.len())
    })
}

fn successful_rg_status(status: ExitStatus) -> bool {
    matches!(status.code(), Some(0 | 1))
}

fn reap(child: &mut Child) {
    let _kill_result = child.kill();
    let _wait_result = child.wait();
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::SearchRunner;

    #[test]
    fn a_zero_timeout_is_available_only_to_deterministic_tests() {
        assert_eq!(
            SearchRunner::new("rg", "unused-driver")
                .with_timeout(Duration::ZERO)
                .timeout,
            Duration::ZERO
        );
    }
}
