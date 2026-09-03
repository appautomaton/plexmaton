//! Strict model schemas, trusted admission, and executor dispatch.

mod arguments;
mod definitions;
mod outcome;

use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmissionRequest, AdmittedToolCall, ToolDefinitionRevision,
    ToolOutcome,
};
use plexmaton_core::{ToolCapability, ToolDefinitionId};

use crate::{
    FileCancellation, FileTools,
    mutation::{self, MutationError},
};
use arguments::{
    canonical_create, canonical_edit, canonical_read, canonical_search, parse_create, parse_edit,
    parse_read, parse_search,
};
use definitions::{
    CREATE_DEFINITION_ID, DEFINITION_REVISION, EDIT_DEFINITION_ID, READ_DEFINITION_ID,
    SEARCH_DEFINITION_ID,
};
pub use definitions::{
    CREATE_TOOL_NAME, EDIT_TOOL_NAME, FileToolDefinition, READ_TOOL_NAME, SEARCH_TOOL_NAME,
};
use outcome::{execute_create, execute_edit, execute_read, execute_search, failed_json};

pub(crate) fn definitions() -> [FileToolDefinition; 4] {
    definitions::definitions()
}

pub(crate) fn admit(
    tools: &FileTools,
    request: AdmissionRequest,
    cancellation: &FileCancellation,
) -> AdmissionOutcome {
    admit_before_resolution(tools, request, cancellation, || {})
}

pub(crate) fn admit_before_resolution(
    tools: &FileTools,
    request: AdmissionRequest,
    cancellation: &FileCancellation,
    before_resolution: impl FnOnce(),
) -> AdmissionOutcome {
    match request.requested().name.as_str() {
        READ_TOOL_NAME => admit_read(request),
        SEARCH_TOOL_NAME => admit_search(request),
        EDIT_TOOL_NAME => admit_edit(tools, request, cancellation, before_resolution),
        CREATE_TOOL_NAME => admit_create(tools, request, cancellation, before_resolution),
        _ => request.refuse(AdmissionRefusal::UnknownTool),
    }
}

fn admit_read(request: AdmissionRequest) -> AdmissionOutcome {
    let Some(arguments) = parse_read(&request.requested().arguments) else {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    };
    admit_call(
        request,
        READ_DEFINITION_ID,
        [ToolCapability::FileRead],
        canonical_read(&arguments),
        format!("read {}", bounded_detail(&arguments.path, 900)),
    )
}

fn admit_search(request: AdmissionRequest) -> AdmissionOutcome {
    let Some(arguments) = parse_search(&request.requested().arguments) else {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    };
    admit_call(
        request,
        SEARCH_DEFINITION_ID,
        [ToolCapability::FileRead],
        canonical_search(&arguments),
        format!(
            "search {} in {}",
            bounded_detail(&arguments.pattern, 400),
            bounded_detail(&arguments.path, 400)
        ),
    )
}

fn admit_edit(
    tools: &FileTools,
    request: AdmissionRequest,
    cancellation: &FileCancellation,
    before_resolution: impl FnOnce(),
) -> AdmissionOutcome {
    let Some(arguments) = parse_edit(&request.requested().arguments) else {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    };
    let canonical =
        match mutation::admit_edit(&tools.root, &tools.observations, &arguments, cancellation) {
            Ok(canonical) => canonical,
            Err(error) => return request.refuse(refusal_for_mutation(&error)),
        };
    let detail = format!(
        "edit {} ({} exact replacement{})",
        bounded_detail(&canonical.path, 800),
        canonical.splices.len(),
        if canonical.splices.len() == 1 {
            ""
        } else {
            "s"
        }
    );
    admit_mutation_call(
        request,
        EDIT_DEFINITION_ID,
        [ToolCapability::FileRead, ToolCapability::FileWrite],
        canonical_edit(&canonical),
        detail,
        cancellation,
        before_resolution,
    )
}

fn admit_create(
    tools: &FileTools,
    request: AdmissionRequest,
    cancellation: &FileCancellation,
    before_resolution: impl FnOnce(),
) -> AdmissionOutcome {
    let Some(arguments) = parse_create(&request.requested().arguments) else {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    };
    let canonical = match mutation::admit_create(&tools.root, &arguments, cancellation) {
        Ok(canonical) => canonical,
        Err(error) => return request.refuse(refusal_for_mutation(&error)),
    };
    let detail = format!("create {}", bounded_detail(&canonical.path, 900));
    admit_mutation_call(
        request,
        CREATE_DEFINITION_ID,
        [ToolCapability::FileWrite],
        canonical_create(&canonical),
        detail,
        cancellation,
        before_resolution,
    )
}

fn admit_mutation_call(
    request: AdmissionRequest,
    definition: &'static str,
    capabilities: impl IntoIterator<Item = ToolCapability>,
    canonical_arguments: Option<String>,
    detail: String,
    cancellation: &FileCancellation,
    before_resolution: impl FnOnce(),
) -> AdmissionOutcome {
    before_resolution();
    if cancellation.is_cancelled() {
        return request.refuse(AdmissionRefusal::Cancelled);
    }
    admit_call(
        request,
        definition,
        capabilities,
        canonical_arguments,
        detail,
    )
}

fn admit_call(
    request: AdmissionRequest,
    definition: &'static str,
    capabilities: impl IntoIterator<Item = ToolCapability>,
    canonical_arguments: Option<String>,
    detail: String,
) -> AdmissionOutcome {
    let Some(definition_id) = ToolDefinitionId::new(definition).ok() else {
        return request.refuse(AdmissionRefusal::DefinitionUnavailable);
    };
    let Some(revision) = ToolDefinitionRevision::new(DEFINITION_REVISION) else {
        return request.refuse(AdmissionRefusal::DefinitionUnavailable);
    };
    let Some(canonical_arguments) = canonical_arguments else {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    };
    let call_id = request.requested().call_id.clone();
    match request.admit(
        definition_id,
        revision,
        capabilities,
        canonical_arguments,
        detail,
    ) {
        Ok(outcome) => outcome,
        Err(_) => AdmissionOutcome::Refused {
            call_id,
            reason: AdmissionRefusal::InvalidArguments,
        },
    }
}

fn refusal_for_mutation(error: &MutationError) -> AdmissionRefusal {
    match error {
        MutationError::StaleObservation
        | MutationError::ChangedBeforeCommit
        | MutationError::CreateCollision => AdmissionRefusal::StalePrecondition,
        MutationError::SourceMismatch => AdmissionRefusal::SourceMismatch,
        MutationError::AmbiguousTarget => AdmissionRefusal::AmbiguousTarget,
        MutationError::OverlappingEdits => AdmissionRefusal::ConflictingArguments,
        MutationError::Cancelled => AdmissionRefusal::Cancelled,
        MutationError::Io(_) | MutationError::StagingChanged => {
            AdmissionRefusal::DefinitionUnavailable
        }
        MutationError::InvalidArguments
        | MutationError::SourceTooLarge
        | MutationError::ResultTooLarge
        | MutationError::InvalidUtf8
        | MutationError::Binary
        | MutationError::Path(_) => AdmissionRefusal::InvalidArguments,
    }
}

pub(crate) fn execute(
    tools: &mut FileTools,
    call: &AdmittedToolCall,
    cancellation: &FileCancellation,
) -> ToolOutcome {
    if call.definition_revision().get() != DEFINITION_REVISION {
        return definition_mismatch();
    }
    let expected_capabilities = match call.definition_id().as_str() {
        READ_DEFINITION_ID | SEARCH_DEFINITION_ID => &[ToolCapability::FileRead][..],
        EDIT_DEFINITION_ID => &[ToolCapability::FileRead, ToolCapability::FileWrite][..],
        CREATE_DEFINITION_ID => &[ToolCapability::FileWrite][..],
        _ => return definition_mismatch(),
    };
    if !call
        .capabilities()
        .iter()
        .eq(expected_capabilities.iter().copied())
    {
        return definition_mismatch();
    }
    match call.definition_id().as_str() {
        READ_DEFINITION_ID => execute_read(call, tools, cancellation),
        SEARCH_DEFINITION_ID => execute_search(call, tools, cancellation),
        EDIT_DEFINITION_ID => execute_edit(call, tools, cancellation),
        CREATE_DEFINITION_ID => execute_create(call, tools, cancellation),
        _ => definition_mismatch(),
    }
}

fn definition_mismatch() -> ToolOutcome {
    failed_json(
        "definition_mismatch",
        "the admitted definition revision or capabilities do not match this catalog",
    )
}

fn bounded_detail(value: &str, max: usize) -> &str {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &value[..end]
}
