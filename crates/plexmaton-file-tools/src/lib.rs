//! Workspace-confined native file inspection and its authoritative observations.

#[cfg(not(unix))]
compile_error!("plexmaton-file-tools currently requires Unix descriptor semantics");

mod catalog;
mod driver;
mod mutation;
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
use plexmaton_agent::{AdmissionOutcome, AdmissionRequest, AdmittedToolCall, ToolOutcome};

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
        let (mut result, version, byte_range) = read::read(&self.root, request)?;
        before_observation();
        if cancellation.is_cancelled() {
            return Err(ReadError::Cancelled);
        }
        result.observation = self
            .observations
            .record(result.path.clone(), version, byte_range);
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
    pub fn definitions() -> [FileToolDefinition; 4] {
        catalog::definitions()
    }

    /// Turns one untrusted model call into immutable arguments and capability facts (WFS-5).
    #[must_use]
    pub fn admit(
        &self,
        request: AdmissionRequest,
        cancellation: &FileCancellation,
    ) -> AdmissionOutcome {
        catalog::admit(self, request, cancellation)
    }

    #[cfg(test)]
    fn admit_before_resolution(
        &self,
        request: AdmissionRequest,
        cancellation: &FileCancellation,
        before_resolution: impl FnOnce(),
    ) -> AdmissionOutcome {
        catalog::admit_before_resolution(self, request, cancellation, before_resolution)
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
pub use catalog::{
    CREATE_TOOL_NAME, EDIT_TOOL_NAME, FileToolDefinition, READ_TOOL_NAME, SEARCH_TOOL_NAME,
};
pub use driver::run_search_driver;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use plexmaton_agent::{
        AdmissionOutcome, AdmissionRefusal, AdmissionRequest, Agent, Effect, Input, ModelEvent,
        StopReason, ToolCall,
    };
    use plexmaton_core::{AgentId, ToolCallId};
    use serde_json::json;

    use super::{EDIT_TOOL_NAME, FileCancellation, FileTools, ReadError, ReadRequest};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn admission_request(name: &str, arguments: serde_json::Value) -> AdmissionRequest {
        let mut agent = Agent::new(
            AgentId::new("file-tool-unit-fixture")
                .unwrap_or_else(|error| panic!("fixture agent ID: {error}")),
        );
        let _submitted = agent.handle(Input::Submitted {
            text: "exercise tool".to_owned(),
        });
        let step_id = agent
            .active_model_step()
            .unwrap_or_else(|| panic!("fixture model step did not open"));
        let _called = agent.handle(Input::Streamed {
            step_id: step_id.clone(),
            event: ModelEvent::Called(ToolCall {
                call_id: ToolCallId::new("unit-call")
                    .unwrap_or_else(|error| panic!("fixture call ID: {error}")),
                name: name.to_owned(),
                arguments: arguments.to_string(),
            }),
        });
        let stopped = agent.handle(Input::Streamed {
            step_id,
            event: ModelEvent::Stopped(StopReason::ToolCalls),
        });
        let mut effects = stopped.effects.into_iter();
        let Some(Effect::AdmitTool(request)) = effects.next() else {
            panic!("fixture did not emit admission");
        };
        assert!(effects.next().is_none());
        request
    }

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

    #[test]
    fn catalog_never_publishes_a_trusted_call_after_final_cancellation() {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "plexmaton-admission-cancel-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir(&directory)
            .unwrap_or_else(|error| panic!("create test directory: {error}"));
        std::fs::write(directory.join("file"), b"old\n")
            .unwrap_or_else(|error| panic!("write fixture: {error}"));
        let mut tools = FileTools::open(&directory, "/bin/false", "/bin/false")
            .unwrap_or_else(|error| panic!("open tools: {error}"));
        let observed = tools
            .read(
                &ReadRequest::new("file".to_owned(), None, None)
                    .unwrap_or_else(|error| panic!("read request: {error}")),
                &FileCancellation::new(),
            )
            .unwrap_or_else(|error| panic!("read fixture: {error}"))
            .observation
            .as_token();
        let cancellation = FileCancellation::new();
        let to_cancel = cancellation.clone();
        let outcome = tools.admit_before_resolution(
            admission_request(
                EDIT_TOOL_NAME,
                json!({"path":"file", "observation":observed, "edits":[{"old_text":"old", "new_text":"new"}]}),
            ),
            &cancellation,
            || to_cancel.cancel(),
        );
        assert!(matches!(
            outcome,
            AdmissionOutcome::Refused {
                reason: AdmissionRefusal::Cancelled,
                ..
            }
        ));
        assert_eq!(
            std::fs::read(directory.join("file"))
                .unwrap_or_else(|error| panic!("read unchanged file: {error}")),
            b"old\n"
        );
        std::fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("remove test directory: {error}"));
    }
}
