mod support;

use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmittedToolCall, ToolCall, ToolDefinitionRevision,
    ToolOutcome,
};
use plexmaton_core::{ToolCallId, ToolCapability};
use plexmaton_file_tools::{FileCancellation, FileTools, READ_TOOL_NAME, SEARCH_TOOL_NAME};
use serde_json::{Value, json};
use support::{TestWorkspace, rg_executable, search_driver};

fn call(name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        call_id: ToolCallId::new(format!("call-{name}"))
            .unwrap_or_else(|error| panic!("call ID: {error}")),
        name: name.to_owned(),
        arguments: arguments.to_string(),
    }
}

fn admitted(outcome: AdmissionOutcome) -> AdmittedToolCall {
    match outcome {
        AdmissionOutcome::Admitted(call) => call,
        AdmissionOutcome::Refused { reason, .. } => panic!("unexpected refusal: {reason:?}"),
    }
}

/// WFS-5: provider-neutral definitions expose strict, bounded object schemas.
#[test]
fn catalog_definitions_are_strict_and_bounded() {
    let definitions = FileTools::definitions();
    assert_eq!(definitions[0].name(), READ_TOOL_NAME);
    assert_eq!(definitions[1].name(), SEARCH_TOOL_NAME);
    for definition in &definitions {
        assert_eq!(definition.parameters()["type"], "object");
        assert_eq!(definition.parameters()["additionalProperties"], false);
    }
    assert_eq!(
        definitions[0].parameters()["properties"]["limit"]["maximum"],
        1000
    );
    assert_eq!(
        definitions[1].parameters()["properties"]["limit"]["maximum"],
        500
    );
}

/// WFS-5: admission rejects unknown structure and freezes canonical defaults and capabilities.
#[test]
fn admission_is_strict_and_canonical() {
    let workspace = TestWorkspace::new();
    let tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let invalid = tools.admit(call(
        READ_TOOL_NAME,
        json!({"path": "file", "surprise": true}),
    ));
    assert!(matches!(
        invalid,
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::InvalidArguments,
            ..
        }
    ));
    let oversized = tools.admit(call(READ_TOOL_NAME, json!({"path": "x".repeat(4097)})));
    assert!(matches!(
        oversized,
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::InvalidArguments,
            ..
        }
    ));
    let null_glob = tools.admit(call(
        SEARCH_TOOL_NAME,
        json!({"pattern": "value", "glob": null}),
    ));
    assert!(matches!(
        null_glob,
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::InvalidArguments,
            ..
        }
    ));

    let read = admitted(tools.admit(call(READ_TOOL_NAME, json!({"path": "file"}))));
    assert_eq!(read.definition_id().as_str(), "native-read-file-v1");
    assert_eq!(read.definition_revision().get(), 1);
    assert_eq!(
        read.capabilities().iter().collect::<Vec<_>>(),
        [ToolCapability::FileRead]
    );
    assert_eq!(
        serde_json::from_str::<Value>(read.canonical_arguments())
            .unwrap_or_else(|error| panic!("canonical arguments: {error}")),
        json!({"path": "file", "offset": 1, "limit": 200})
    );
    let search = admitted(tools.admit(call(SEARCH_TOOL_NAME, json!({"pattern": "value"}))));
    assert_eq!(search.definition_id().as_str(), "native-search-v1");
    assert_eq!(
        serde_json::from_str::<Value>(search.canonical_arguments())
            .unwrap_or_else(|error| panic!("canonical arguments: {error}")),
        json!({"pattern": "value", "path": ".", "limit": 100})
    );
}

/// WFS-5: execution trusts the admitted definition identity, never the requested model name.
#[test]
fn execution_dispatches_by_admitted_definition() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"exact\r\n");
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let admitted = admitted(tools.admit(call(READ_TOOL_NAME, json!({"path": "file"}))));
    let mismatched_name = AdmittedToolCall::new(
        call(SEARCH_TOOL_NAME, json!({"pattern": "ignored"})),
        admitted.definition_id().clone(),
        admitted.definition_revision(),
        admitted.capabilities().iter(),
        admitted.canonical_arguments().to_owned(),
        admitted.detail().to_owned(),
    )
    .unwrap_or_else(|error| panic!("admitted fixture: {error:?}"));

    let outcome = tools.execute(&mismatched_name, &FileCancellation::new());
    let ToolOutcome::Succeeded { output } = outcome else {
        panic!("read definition did not execute: {outcome:?}");
    };
    let output: Value =
        serde_json::from_str(&output).unwrap_or_else(|error| panic!("tool output: {error}"));
    assert_eq!(output["content"], "exact\r\n");
    assert_eq!(output["completion"], "end_of_file");
}

/// WFS-5: stale revisions and forged capability facts cannot reuse a current executor.
#[test]
fn execution_rechecks_revision_and_capabilities() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"exact\n");
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let admitted = admitted(tools.admit(call(READ_TOOL_NAME, json!({"path": "file"}))));

    for (revision, capabilities) in [
        (
            ToolDefinitionRevision::new(2).unwrap_or_else(|| panic!("fixture revision")),
            vec![ToolCapability::FileRead],
        ),
        (
            admitted.definition_revision(),
            vec![ToolCapability::FileRead, ToolCapability::FileWrite],
        ),
    ] {
        let forged = AdmittedToolCall::new(
            admitted.requested().clone(),
            admitted.definition_id().clone(),
            revision,
            capabilities,
            admitted.canonical_arguments().to_owned(),
            admitted.detail().to_owned(),
        )
        .unwrap_or_else(|error| panic!("admitted fixture: {error:?}"));
        assert!(matches!(
            tools.execute(&forged, &FileCancellation::new()),
            ToolOutcome::Failed { .. }
        ));
    }
}

/// WFS-5: canonical search arguments are accepted by the same strict executor parser.
#[test]
fn search_execution_accepts_its_own_canonical_arguments() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"needle\n");
    let mut tools = FileTools::open(workspace.path(), rg_executable(), search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let call = admitted(tools.admit(call(SEARCH_TOOL_NAME, json!({"pattern": "needle"}))));

    let outcome = tools.execute(&call, &FileCancellation::new());
    let ToolOutcome::Succeeded { output } = outcome else {
        panic!("search definition did not execute: {outcome:?}");
    };
    let output: Value =
        serde_json::from_str(&output).unwrap_or_else(|error| panic!("tool output: {error}"));
    assert_eq!(output["matches"][0]["path"], "file");
    assert_eq!(output["matches"][0]["preview"], "needle");
}
