//! Strict model schemas and trusted admission into concrete file operations.

use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmittedToolCall, ToolCall, ToolDefinitionRevision,
    ToolOutcome,
};
use plexmaton_core::{ToolCapability, ToolDefinitionId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    FileCancellation, FileTools, ReadCompletion, ReadError, ReadRequest, SearchCompletion,
    SearchError, SearchRequest,
};

pub const READ_TOOL_NAME: &str = "read_file";
pub const SEARCH_TOOL_NAME: &str = "search";
const READ_DEFINITION_ID: &str = "native-read-file-v1";
const SEARCH_DEFINITION_ID: &str = "native-search-v1";
const DEFINITION_REVISION: u64 = 1;

/// Provider-neutral strict function definition.
#[derive(Clone, Debug, PartialEq)]
pub struct FileToolDefinition {
    name: &'static str,
    description: &'static str,
    parameters: Value,
}

impl FileToolDefinition {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[must_use]
    pub const fn description(&self) -> &'static str {
        self.description
    }

    #[must_use]
    pub const fn parameters(&self) -> &Value {
        &self.parameters
    }
}

pub(crate) fn definitions() -> [FileToolDefinition; 2] {
    [
        FileToolDefinition {
            name: READ_TOOL_NAME,
            description: "Read an exact UTF-8 line window from a file in the workspace. Returns an opaque observation required for later edits and a next offset when more text remains.",
            parameters: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative file path." },
                    "offset": { "type": "integer", "minimum": 1, "description": "First line to return. Defaults to 1." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Maximum lines to return. Defaults to 200." }
                },
                "required": ["path"]
            }),
        },
        FileToolDefinition {
            name: SEARCH_TOOL_NAME,
            description: "Search UTF-8 text in the workspace with ripgrep. Returns bounded file/line previews; refine the query when a result bound is reached.",
            parameters: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "pattern": { "type": "string", "description": "Regular expression to search for." },
                    "path": { "type": "string", "description": "Workspace-relative file or directory. Defaults to the workspace root." },
                    "glob": { "type": "string", "description": "Optional ripgrep glob filter for a directory search; omit when path names a file." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500, "description": "Maximum matches to return. Defaults to 100." }
                },
                "required": ["pattern"]
            }),
        },
    ]
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReadArguments {
    path: String,
    #[serde(default = "default_read_offset")]
    offset: u64,
    #[serde(default = "default_read_limit")]
    limit: u16,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SearchArguments {
    pattern: String,
    #[serde(default = "default_search_path")]
    path: String,
    #[serde(
        default,
        deserialize_with = "deserialize_present_glob",
        skip_serializing_if = "Option::is_none"
    )]
    glob: Option<String>,
    #[serde(default = "default_search_limit")]
    limit: u16,
}

const fn default_read_offset() -> u64 {
    1
}

const fn default_read_limit() -> u16 {
    200
}

fn default_search_path() -> String {
    ".".to_owned()
}

fn deserialize_present_glob<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

const fn default_search_limit() -> u16 {
    100
}

pub(crate) fn admit(call: ToolCall) -> AdmissionOutcome {
    let admitted = match call.name.as_str() {
        READ_TOOL_NAME => admit_read(call.clone()),
        SEARCH_TOOL_NAME => admit_search(call.clone()),
        _ => return refused(call, AdmissionRefusal::UnknownTool),
    };
    admitted.unwrap_or_else(|| refused(call, AdmissionRefusal::InvalidArguments))
}

fn admit_read(call: ToolCall) -> Option<AdmissionOutcome> {
    let arguments: ReadArguments = serde_json::from_str(&call.arguments).ok()?;
    ReadRequest::new(
        arguments.path.clone(),
        Some(arguments.offset),
        Some(arguments.limit),
    )
    .ok()?;
    admitted(
        call,
        READ_DEFINITION_ID,
        serde_json::to_string(&arguments).ok()?,
        format!("read {}", bounded_detail(&arguments.path, 900)),
    )
}

fn admit_search(call: ToolCall) -> Option<AdmissionOutcome> {
    let arguments: SearchArguments = serde_json::from_str(&call.arguments).ok()?;
    SearchRequest::new(
        arguments.pattern.clone(),
        Some(arguments.path.clone()),
        arguments.glob.clone(),
        Some(arguments.limit),
    )
    .ok()?;
    admitted(
        call,
        SEARCH_DEFINITION_ID,
        serde_json::to_string(&arguments).ok()?,
        format!(
            "search {} in {}",
            bounded_detail(&arguments.pattern, 400),
            bounded_detail(&arguments.path, 400)
        ),
    )
}

fn admitted(
    call: ToolCall,
    definition: &'static str,
    canonical_arguments: String,
    detail: String,
) -> Option<AdmissionOutcome> {
    let definition_id = ToolDefinitionId::new(definition).ok()?;
    let revision = ToolDefinitionRevision::new(DEFINITION_REVISION)?;
    AdmittedToolCall::new(
        call,
        definition_id,
        revision,
        [ToolCapability::FileRead],
        canonical_arguments,
        detail,
    )
    .ok()
    .map(AdmissionOutcome::Admitted)
}

fn refused(call: ToolCall, reason: AdmissionRefusal) -> AdmissionOutcome {
    AdmissionOutcome::Refused {
        call_id: call.call_id,
        reason,
    }
}

fn bounded_detail(value: &str, max: usize) -> &str {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &value[..end]
}

pub(crate) fn execute(
    tools: &mut FileTools,
    call: &AdmittedToolCall,
    cancellation: &FileCancellation,
) -> ToolOutcome {
    if call.definition_revision().get() != DEFINITION_REVISION
        || !call.capabilities().iter().eq([ToolCapability::FileRead])
    {
        return failed_json(
            "definition_mismatch",
            "the admitted definition revision or capabilities do not match this catalog",
        );
    }
    match call.definition_id().as_str() {
        READ_DEFINITION_ID => execute_read(to_read_request(call), tools, cancellation),
        SEARCH_DEFINITION_ID => execute_search(to_search_request(call), tools, cancellation),
        _ => Err(failed_json(
            "definition_mismatch",
            "the admitted definition is not a file tool",
        )),
    }
    .unwrap_or_else(|error| error)
}

fn execute_read(
    request: Result<ReadRequest, ToolOutcome>,
    tools: &mut FileTools,
    cancellation: &FileCancellation,
) -> Result<ToolOutcome, ToolOutcome> {
    let request = request?;
    match tools.read(&request, cancellation) {
        Ok(result) => Ok(succeeded_json(json!({
            "path": result.path,
            "observation": result.observation.as_token(),
            "start_line": result.start_line,
            "content": result.content,
            "lines": result.lines,
            "completion": read_completion(result.completion),
            "next_offset": result.next_offset,
        }))),
        Err(error) => Err(failed_json(error.kind(), &error.to_string())),
    }
}

fn execute_search(
    request: Result<SearchRequest, ToolOutcome>,
    tools: &FileTools,
    cancellation: &FileCancellation,
) -> Result<ToolOutcome, ToolOutcome> {
    let request = request?;
    match tools.search(&request, cancellation) {
        Ok(result) => Ok(succeeded_json(json!({
            "matches": result.matches.iter().map(|found| json!({
                "path": found.path,
                "line": found.line,
                "preview": found.preview,
                "preview_truncated": found.preview_truncated,
            })).collect::<Vec<_>>(),
            "completion": search_completion(result.completion),
            "matches_seen": result.matches_seen,
        }))),
        Err(error) => Err(failed_json(error.kind(), &error.to_string())),
    }
}

fn to_read_request(call: &AdmittedToolCall) -> Result<ReadRequest, ToolOutcome> {
    let arguments: ReadArguments =
        serde_json::from_str(call.canonical_arguments()).map_err(|_| {
            failed_json(
                "canonical_arguments",
                "canonical read arguments are invalid",
            )
        })?;
    ReadRequest::new(
        arguments.path,
        Some(arguments.offset),
        Some(arguments.limit),
    )
    .map_err(|error| failed_json(error.kind(), &error.to_string()))
}

fn to_search_request(call: &AdmittedToolCall) -> Result<SearchRequest, ToolOutcome> {
    let arguments: SearchArguments =
        serde_json::from_str(call.canonical_arguments()).map_err(|_| {
            failed_json(
                "canonical_arguments",
                "canonical search arguments are invalid",
            )
        })?;
    SearchRequest::new(
        arguments.pattern,
        Some(arguments.path),
        arguments.glob,
        Some(arguments.limit),
    )
    .map_err(|error| failed_json(error.kind(), &error.to_string()))
}

fn succeeded_json(value: Value) -> ToolOutcome {
    ToolOutcome::Succeeded {
        output: value.to_string(),
    }
}

fn failed_json(kind: &str, message: &str) -> ToolOutcome {
    ToolOutcome::Failed {
        message: json!({ "kind": kind, "message": message }).to_string(),
    }
}

fn read_completion(completion: ReadCompletion) -> &'static str {
    match completion {
        ReadCompletion::EndOfFile => "end_of_file",
        ReadCompletion::LineLimit => "line_limit",
        ReadCompletion::ByteLimit => "byte_limit",
    }
}

fn search_completion(completion: SearchCompletion) -> &'static str {
    match completion {
        SearchCompletion::Complete => "complete",
        SearchCompletion::FileLimit => "file_limit",
        SearchCompletion::FileByteLimit => "file_byte_limit",
        SearchCompletion::MatchLimit => "match_limit",
        SearchCompletion::TransportByteLimit => "transport_byte_limit",
        SearchCompletion::RetainedByteLimit => "retained_byte_limit",
    }
}

impl ReadError {
    fn kind(&self) -> &'static str {
        match self {
            Self::InvalidWindow => "invalid_window",
            Self::Path(_) => "path",
            Self::Io(_) => "io",
            Self::InvalidUtf8 { .. } => "invalid_utf8",
            Self::Binary { .. } => "binary",
            Self::LineTooLong { .. } => "line_too_long",
            Self::ScanLimit { .. } => "scan_limit",
            Self::ChangedDuringRead => "changed_during_read",
            Self::Cancelled => "cancelled",
        }
    }
}

impl SearchError {
    fn kind(&self) -> &'static str {
        match self {
            Self::InvalidArguments => "invalid_arguments",
            Self::Path(_) => "path",
            Self::Spawn(_) => "spawn",
            Self::UntrustedExecutable => "untrusted_executable",
            Self::Io(_) => "io",
            Self::RecordTooLarge { .. } => "record_too_large",
            Self::CandidateTooLong { .. } => "candidate_too_long",
            Self::InvalidProtocol => "invalid_protocol",
            Self::ChangedDuringSearch => "changed_during_search",
            Self::ProcessFailed { .. } => "process_failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::ReaderFailed => "reader_failed",
        }
    }
}
