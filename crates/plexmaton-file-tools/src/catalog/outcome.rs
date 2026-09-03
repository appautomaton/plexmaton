use plexmaton_agent::{AdmittedToolCall, ToolExecutionResult, ToolOutcome, bounded_tool_text};
use plexmaton_core::ToolDetail;
use serde_json::{Value, json};

use crate::{
    FileCancellation, FileTools, ReadCompletion, ReadError, ReadRequest, SearchCompletion,
    SearchError, SearchRequest,
    mutation::{self, CreateArguments, MutationError},
};

use super::arguments::{parse_canonical_edit, parse_create, parse_read, parse_search};

pub(super) fn execute_read(
    call: &AdmittedToolCall,
    tools: &mut FileTools,
    cancellation: &FileCancellation,
) -> ToolExecutionResult {
    let Some(arguments) = parse_read(call.canonical_arguments()) else {
        return canonical_failure("read");
    };
    let request = ReadRequest::new(
        arguments.path,
        Some(arguments.offset),
        Some(arguments.limit),
    );
    let Ok(request) = request else {
        return canonical_failure("read");
    };
    match tools.read(&request, cancellation) {
        Ok(result) => succeeded_json(json!({
            "path": result.path,
            "observation": result.observation.as_token(),
            "start_line": result.start_line,
            "content": result.content,
            "lines": result.lines,
            "completion": read_completion(result.completion),
            "next_offset": result.next_offset,
        })),
        Err(error) => failed_json(error.kind(), &error.to_string()),
    }
}

pub(super) fn execute_search(
    call: &AdmittedToolCall,
    tools: &FileTools,
    cancellation: &FileCancellation,
) -> ToolExecutionResult {
    let Some(arguments) = parse_search(call.canonical_arguments()) else {
        return canonical_failure("search");
    };
    let request = SearchRequest::new(
        arguments.pattern,
        Some(arguments.path),
        arguments.glob,
        Some(arguments.limit),
    );
    let Ok(request) = request else {
        return canonical_failure("search");
    };
    match tools.search(&request, cancellation) {
        Ok(result) => succeeded_json(json!({
            "matches": result.matches.iter().map(|found| json!({
                "path": found.path,
                "line": found.line,
                "preview": found.preview,
                "preview_truncated": found.preview_truncated,
            })).collect::<Vec<_>>(),
            "completion": search_completion(result.completion),
            "matches_seen": result.matches_seen,
        })),
        Err(error) => failed_json(error.kind(), &error.to_string()),
    }
}

pub(super) fn execute_edit(
    call: &AdmittedToolCall,
    tools: &FileTools,
    cancellation: &FileCancellation,
) -> ToolExecutionResult {
    let Some(canonical) = parse_canonical_edit(call.canonical_arguments()) else {
        return canonical_failure("edit");
    };
    let path = canonical.path.clone();
    match mutation::execute_edit(&tools.root, &tools.observations, canonical, cancellation) {
        Ok(applied) => {
            let output = json!({
                "path": path,
                "edits_applied": applied.edits_applied,
            })
            .to_string();
            ToolExecutionResult::new(
                ToolOutcome::Succeeded { output },
                Some(ToolDetail::Diff {
                    patch: applied.patch,
                }),
            )
        }
        Err(error) => mutation_failure(&error),
    }
}

pub(super) fn execute_create(
    call: &AdmittedToolCall,
    tools: &FileTools,
    cancellation: &FileCancellation,
) -> ToolExecutionResult {
    let Some(arguments): Option<CreateArguments> = parse_create(call.canonical_arguments()) else {
        return canonical_failure("create");
    };
    match mutation::execute_create(&tools.root, &arguments, cancellation) {
        Ok(bytes_written) => succeeded_json(json!({
            "path": arguments.path,
            "bytes_written": bytes_written,
        })),
        Err(error) => mutation_failure(&error),
    }
}

fn mutation_failure(error: &MutationError) -> ToolExecutionResult {
    failed_json(error.kind(), &error.to_string())
}

fn canonical_failure(operation: &str) -> ToolExecutionResult {
    failed_json(
        "canonical_arguments",
        &format!("canonical {operation} arguments are invalid"),
    )
}

fn succeeded_json(value: Value) -> ToolExecutionResult {
    let output = value.to_string();
    let presentation = Some(bounded_tool_text(&output, 0));
    ToolExecutionResult::new(ToolOutcome::Succeeded { output }, presentation)
}

pub(super) fn failed_json(kind: &str, message: &str) -> ToolExecutionResult {
    let message = json!({ "kind": kind, "message": message }).to_string();
    let presentation = Some(bounded_tool_text(&message, 0));
    ToolExecutionResult::new(ToolOutcome::Failed { message }, presentation)
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
