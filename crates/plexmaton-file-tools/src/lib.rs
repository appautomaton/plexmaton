//! Workspace-confined native file inspection and its authoritative observations.

#[cfg(not(unix))]
compile_error!("plexmaton-file-tools currently requires Unix descriptor semantics");

mod catalog;
mod driver;
mod observation;
mod path;
mod read;
mod search;

pub use observation::{ObservationId, ObservedFile};
pub use path::{PathError, WorkspaceRoot};
pub use read::{ReadCompletion, ReadError, ReadRequest, ReadResult};
pub use search::{
    FileCancellation, MAX_SEARCH_FILE_BYTES, MAX_SEARCH_FILES, MAX_SEARCH_RESULT_BYTES,
    SearchCompletion, SearchError, SearchMatch, SearchRequest, SearchResult, SearchRunner,
};

use observation::ObservationStore;
use plexmaton_agent::{AdmissionOutcome, AdmittedToolCall, ToolCall, ToolOutcome};

/// One workspace filesystem boundary and the bounded observations it has issued.
pub struct FileTools {
    root: WorkspaceRoot,
    observations: ObservationStore,
    search: SearchRunner,
}

impl FileTools {
    /// Pins one existing directory as the only root these tools may inspect.
    pub fn open(
        root: impl AsRef<std::path::Path>,
        executable: impl Into<std::path::PathBuf>,
        directory_driver: impl Into<std::path::PathBuf>,
    ) -> Result<Self, PathError> {
        Ok(Self {
            root: WorkspaceRoot::open(root)?,
            observations: ObservationStore::default(),
            search: SearchRunner::new(executable, directory_driver),
        })
    }

    /// Reads one exact bounded text window and records the version read from its open handle.
    pub fn read(
        &mut self,
        request: &ReadRequest,
        cancellation: &FileCancellation,
    ) -> Result<ReadResult, ReadError> {
        self.read_before_observation(request, cancellation, || {})
    }

    fn read_before_observation(
        &mut self,
        request: &ReadRequest,
        cancellation: &FileCancellation,
        before_observation: impl FnOnce(),
    ) -> Result<ReadResult, ReadError> {
        if cancellation.is_cancelled() {
            return Err(ReadError::Cancelled);
        }
        let (mut result, version) = read::read(&self.root, request)?;
        before_observation();
        if cancellation.is_cancelled() {
            return Err(ReadError::Cancelled);
        }
        result.observation = self.observations.record(result.path.clone(), version);
        Ok(result)
    }

    /// Resolves an opaque observation for Slice 8's integrity precondition.
    #[must_use]
    pub fn observation(&self, id: ObservationId) -> Option<&ObservedFile> {
        self.observations.get(id)
    }

    /// Searches with a directly spawned, bounded ripgrep child.
    pub fn search(
        &self,
        request: &SearchRequest,
        cancellation: &FileCancellation,
    ) -> Result<SearchResult, SearchError> {
        self.search.search(&self.root, request, cancellation)
    }

    /// Canonical root pinned by this owner.
    #[must_use]
    pub fn root(&self) -> &WorkspaceRoot {
        &self.root
    }

    /// Strict model-facing definitions translated by provider adapters only at their wire edge.
    #[must_use]
    pub fn definitions() -> [FileToolDefinition; 2] {
        catalog::definitions()
    }

    /// Turns one untrusted model call into immutable arguments and capability facts (WFS-5).
    #[must_use]
    pub fn admit(&self, call: ToolCall) -> AdmissionOutcome {
        catalog::admit(call)
    }

    /// Executes only a definition identity and canonical arguments produced by admission.
    pub fn execute(
        &mut self,
        call: &AdmittedToolCall,
        cancellation: &FileCancellation,
    ) -> ToolOutcome {
        catalog::execute(self, call, cancellation)
    }
}
pub use catalog::{FileToolDefinition, READ_TOOL_NAME, SEARCH_TOOL_NAME};
pub use driver::run_search_driver;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{FileCancellation, FileTools, ReadError, ReadRequest};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn cancellation_after_read_work_still_publishes_no_observation() {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "plexmaton-read-cancel-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir(&directory)
            .unwrap_or_else(|error| panic!("create test directory: {error}"));
        std::fs::write(directory.join("file"), b"value\n")
            .unwrap_or_else(|error| panic!("write fixture: {error}"));
        let mut tools = FileTools::open(&directory, "/bin/false", "/bin/false")
            .unwrap_or_else(|error| panic!("open tools: {error}"));
        let request = ReadRequest::new("file".to_owned(), None, None)
            .unwrap_or_else(|error| panic!("request: {error}"));
        let cancellation = FileCancellation::new();
        let to_cancel = cancellation.clone();

        assert_eq!(
            tools.read_before_observation(&request, &cancellation, || to_cancel.cancel()),
            Err(ReadError::Cancelled)
        );
        let result = tools
            .read(&request, &FileCancellation::new())
            .unwrap_or_else(|error| panic!("uncancelled read: {error}"));
        assert_eq!(result.observation.as_token(), "obs-0000000000000001");
        std::fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("remove test directory: {error}"));
    }
}
