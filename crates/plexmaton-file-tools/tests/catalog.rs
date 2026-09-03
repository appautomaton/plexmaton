mod support;

use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmittedToolCall, ToolDefinitionRevision, ToolOutcome,
};
use plexmaton_core::{ToolCapability, ToolDetail};
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
        definitions[0].parameters()["properties"]["path"]["minLength"],
        1
    );
    for definition in &definitions {
        assert_eq!(
            definition.parameters()["properties"]["path"]["maxLength"],
            4096
        );
        assert!(
            definition.parameters()["properties"]["path"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("4 KiB of UTF-8 bytes"))
        );
    }
    assert_eq!(
        definitions[1].parameters()["properties"]["pattern"]["maxLength"],
        8192
    );
    assert!(
        definitions[1].parameters()["properties"]["pattern"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("8 KiB of UTF-8 bytes"))
    );
    assert_eq!(
        definitions[1].parameters()["properties"]["glob"]["maxLength"],
        4096
    );
    assert!(
        definitions[1].parameters()["properties"]["glob"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("4 KiB of UTF-8 bytes"))
    );
    assert_eq!(
        definitions[2].parameters()["properties"]["edits"]["maxItems"],
        16
    );
    assert_eq!(
        definitions[2].parameters()["properties"]["edits"]["items"]["properties"]["old_text"]["maxLength"],
        49_152
    );
    assert!(
        definitions[2].parameters()["properties"]["edits"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("48 KiB of UTF-8 bytes"))
    );
    assert_eq!(
        definitions[3].parameters()["properties"]["content"]["maxLength"],
        49_152
    );
    assert!(
        definitions[3].parameters()["properties"]["content"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("48 KiB of UTF-8 bytes"))
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
    assert!(matches!(
        read.invocation(),
        Some(ToolDetail::Text { source, omitted_bytes: 0 })
            if source == "path: \"file\"\nstart_line: 1\nline_limit: 200"
    ));
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
    assert!(matches!(
        search.invocation(),
        Some(ToolDetail::Text { source, omitted_bytes: 0 })
            if source
                == "pattern: \"value\"\npath: \".\"\nglob: null\nmatch_limit: 100"
    ));
}

/// WFS-5/ENT-4: admission freezes one lexical workspace path spelling for provider replay,
/// transcript presentation, and the executor's model-facing result.
#[test]
fn read_and_search_paths_agree_across_canonical_invocation_and_execution() {
    let workspace = TestWorkspace::new();
    workspace.write("nested/notes.txt", b"needle\n");
    let mut tools = FileTools::open(workspace.path(), rg_executable(), search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));

    let read = admitted(tools.admit(
        call(READ_TOOL_NAME, json!({"path": "./nested//notes.txt"})),
        &FileCancellation::new(),
    ));
    let read_arguments: Value = serde_json::from_str(read.canonical_arguments())
        .unwrap_or_else(|error| panic!("canonical read: {error}"));
    assert_eq!(read_arguments["path"], "nested/notes.txt");
    assert_eq!(read.detail(), "read nested/notes.txt");
    assert!(matches!(
        read.invocation(),
        Some(ToolDetail::Text { source, omitted_bytes: 0 })
            if source.contains("path: \"nested/notes.txt\"")
    ));
    let read_result = tools.execute(&read, &FileCancellation::new());
    let ToolOutcome::Succeeded { output } = read_result.outcome() else {
        panic!("canonical read failed: {read_result:?}");
    };
    let read_output: Value =
        serde_json::from_str(output).unwrap_or_else(|error| panic!("read output: {error}"));
    assert_eq!(read_output["path"], read_arguments["path"]);

    let search = admitted(tools.admit(
        call(
            SEARCH_TOOL_NAME,
            json!({"pattern": "needle", "path": "./nested///"}),
        ),
        &FileCancellation::new(),
    ));
    let search_arguments: Value = serde_json::from_str(search.canonical_arguments())
        .unwrap_or_else(|error| panic!("canonical search: {error}"));
    assert_eq!(search_arguments["path"], "nested");
    assert_eq!(search.detail(), "search needle in nested");
    assert!(matches!(
        search.invocation(),
        Some(ToolDetail::Text { source, omitted_bytes: 0 })
            if source.contains("path: \"nested\"")
    ));
    let search_result = tools.execute(&search, &FileCancellation::new());
    let ToolOutcome::Succeeded { output } = search_result.outcome() else {
        panic!("canonical search failed: {search_result:?}");
    };
    let search_output: Value =
        serde_json::from_str(output).unwrap_or_else(|error| panic!("search output: {error}"));
    assert_eq!(search_output["matches"][0]["path"], "nested/notes.txt");
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
                admitted_call.invocation().cloned(),
            )
            .unwrap_or_else(|error| panic!("admitted fixture: {error:?}")),
    );

    let result = tools.execute(&mismatched_name, &FileCancellation::new());
    let ToolOutcome::Succeeded { output } = result.outcome() else {
        panic!("read definition did not execute: {result:?}");
    };
    let output: Value =
        serde_json::from_str(output).unwrap_or_else(|error| panic!("tool output: {error}"));
    assert_eq!(output["content"], "exact\r\n");
    assert_eq!(output["completion"], "end_of_file");
    assert!(matches!(
        result.presentation(),
        Some(ToolDetail::Text { source, omitted_bytes: 0 })
            if serde_json::from_str::<Value>(source).ok().as_ref() == Some(&output)
    ));
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
                    admitted_call.invocation().cloned(),
                )
                .unwrap_or_else(|error| panic!("admitted fixture: {error:?}")),
        );
        assert!(matches!(
            tools.execute(&forged, &FileCancellation::new()).outcome(),
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

    let result = tools.execute(&call, &FileCancellation::new());
    let ToolOutcome::Succeeded { output } = result.outcome() else {
        panic!("search definition did not execute: {result:?}");
    };
    let output: Value =
        serde_json::from_str(output).unwrap_or_else(|error| panic!("tool output: {error}"));
    assert_eq!(output["matches"][0]["path"], "file");
    assert_eq!(output["matches"][0]["preview"], "needle");
    assert!(matches!(
        result.presentation(),
        Some(ToolDetail::Text { source, omitted_bytes: 0 })
            if serde_json::from_str::<Value>(source).ok().as_ref() == Some(&output)
    ));
}
