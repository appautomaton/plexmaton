mod support;

use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmittedToolCall, ToolDefinitionRevision, ToolOutcome,
};
use plexmaton_core::ToolCapability;
use plexmaton_file_tools::{
    CREATE_TOOL_NAME, EDIT_TOOL_NAME, FileCancellation, FileTools, READ_TOOL_NAME, SEARCH_TOOL_NAME,
};
use serde_json::{Value, json};
use support::{TestWorkspace, admission_request as call, rg_executable, search_driver};

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
    assert_eq!(definitions[2].name(), EDIT_TOOL_NAME);
    assert_eq!(definitions[3].name(), CREATE_TOOL_NAME);
    for definition in &definitions {
        assert_eq!(definition.parameters()["type"], "object");
        assert_eq!(definition.parameters()["additionalProperties"], false);
        let property_count = definition.parameters()["properties"]
            .as_object()
            .map(serde_json::Map::len);
        let required_count = definition.parameters()["required"].as_array().map(Vec::len);
        assert_eq!(required_count, property_count);
    }
    assert_eq!(
        definitions[0].parameters()["properties"]["limit"]["maximum"],
        1000
    );
    assert_eq!(
        definitions[1].parameters()["properties"]["limit"]["maximum"],
        500
    );
    assert_eq!(
        definitions[2].parameters()["properties"]["edits"]["maxItems"],
        16
    );
    assert_eq!(
        definitions[3].parameters()["properties"]["content"]["maxLength"],
        49_152
    );
}

/// WFS-5: admission rejects unknown structure and freezes canonical defaults and capabilities.
#[test]
fn admission_is_strict_and_canonical() {
    let workspace = TestWorkspace::new();
    let tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let invalid = tools.admit(
        call(READ_TOOL_NAME, json!({"path": "file", "surprise": true})),
        &FileCancellation::new(),
    );
    assert!(matches!(
        invalid,
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::InvalidArguments,
            ..
        }
    ));
    let oversized = tools.admit(
        call(READ_TOOL_NAME, json!({"path": "x".repeat(4097)})),
        &FileCancellation::new(),
    );
    assert!(matches!(
        oversized,
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::InvalidArguments,
            ..
        }
    ));
    let nullable_defaults = tools.admit(
        call(SEARCH_TOOL_NAME, json!({"pattern": "value", "glob": null})),
        &FileCancellation::new(),
    );
    let nullable_defaults = admitted(nullable_defaults);
    assert_eq!(
        serde_json::from_str::<Value>(nullable_defaults.canonical_arguments())
            .unwrap_or_else(|error| panic!("nullable defaults: {error}")),
        json!({"pattern": "value", "path": ".", "limit": 100})
    );

    let read = admitted(tools.admit(
        call(
            READ_TOOL_NAME,
            json!({"path": "file", "offset": null, "limit": null}),
        ),
        &FileCancellation::new(),
    ));
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
    let search = admitted(tools.admit(
        call(SEARCH_TOOL_NAME, json!({"pattern": "value"})),
        &FileCancellation::new(),
    ));
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
    let admitted_call = admitted(tools.admit(
        call(READ_TOOL_NAME, json!({"path": "file"})),
        &FileCancellation::new(),
    ));
    let mismatched_name = admitted(
        call(SEARCH_TOOL_NAME, json!({"pattern": "ignored"}))
            .admit(
                admitted_call.definition_id().clone(),
                admitted_call.definition_revision(),
                admitted_call.capabilities().iter(),
                admitted_call.canonical_arguments().to_owned(),
                admitted_call.detail().to_owned(),
            )
            .unwrap_or_else(|error| panic!("admitted fixture: {error:?}")),
    );

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
    let admitted_call = admitted(tools.admit(
        call(READ_TOOL_NAME, json!({"path": "file"})),
        &FileCancellation::new(),
    ));

    for (revision, capabilities) in [
        (
            ToolDefinitionRevision::new(2).unwrap_or_else(|| panic!("fixture revision")),
            vec![ToolCapability::FileRead],
        ),
        (
            admitted_call.definition_revision(),
            vec![ToolCapability::FileRead, ToolCapability::FileWrite],
        ),
    ] {
        let forged = admitted(
            call(READ_TOOL_NAME, json!({"path": "file"}))
                .admit(
                    admitted_call.definition_id().clone(),
                    revision,
                    capabilities,
                    admitted_call.canonical_arguments().to_owned(),
                    admitted_call.detail().to_owned(),
                )
                .unwrap_or_else(|error| panic!("admitted fixture: {error:?}")),
        );
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
    let call = admitted(tools.admit(
        call(SEARCH_TOOL_NAME, json!({"pattern": "needle"})),
        &FileCancellation::new(),
    ));

    let outcome = tools.execute(&call, &FileCancellation::new());
    let ToolOutcome::Succeeded { output } = outcome else {
        panic!("search definition did not execute: {outcome:?}");
    };
    let output: Value =
        serde_json::from_str(&output).unwrap_or_else(|error| panic!("tool output: {error}"));
    assert_eq!(output["matches"][0]["path"], "file");
    assert_eq!(output["matches"][0]["preview"], "needle");
}
