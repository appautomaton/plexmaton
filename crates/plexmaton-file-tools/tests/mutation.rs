mod support;

use std::fs;

use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmittedToolCall, ToolExecutionResult, ToolOutcome,
};
use plexmaton_core::ToolCapability;
use plexmaton_core::ToolDetail;
use plexmaton_file_tools::{
    CREATE_TOOL_NAME, EDIT_TOOL_NAME, FileCancellation, FileTools, ReadRequest,
};
use serde_json::{Value, json};
use support::{TestWorkspace, admission_request as call};

fn admitted(outcome: AdmissionOutcome) -> AdmittedToolCall {
    match outcome {
        AdmissionOutcome::Admitted(call) => call,
        AdmissionOutcome::Refused { reason, .. } => panic!("unexpected refusal: {reason:?}"),
    }
}

fn observation(tools: &mut FileTools, path: &str, offset: u64, limit: u16) -> String {
    let request = ReadRequest::new(path.to_owned(), Some(offset), Some(limit))
        .unwrap_or_else(|error| panic!("read request: {error}"));
    tools
        .read(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("read fixture: {error}"))
        .observation
        .as_token()
}

fn failure_kind(result: ToolExecutionResult) -> String {
    let ToolOutcome::Failed { message } = result.outcome() else {
        panic!("expected tool failure, got {result:?}");
    };
    serde_json::from_str::<Value>(message).unwrap_or_else(|error| panic!("failure JSON: {error}"))
        ["kind"]
        .as_str()
        .unwrap_or_else(|| panic!("failure kind missing"))
        .to_owned()
}

/// MUT-1/MUT-4: one observed exact batch compiles to byte splices and preserves all untouched
/// bytes and permission bits.
#[test]
fn exact_batch_preserves_byte_shape_and_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let workspace = TestWorkspace::new();
    let original = "\u{feff}pub const TITLE: &str = \"Plexmaton — alpha\";\r\n\tlet path = r#\"C:\\temp\\old\"#;\r\n#[deprecated]\npub fn legacy() {}\r\npub const KEEP: &str = \"“untouched”\";\r\n";
    let expected = "\u{feff}pub const TITLE: &str = \"Plexmaton — beta\";\r\n\tlet path = r#\"C:\\temp\\new\"#;\r\npub const KEEP: &str = \"“untouched”\";\r\n";
    workspace.write("shape.rs", original.as_bytes());
    fs::set_permissions(
        workspace.path().join("shape.rs"),
        fs::Permissions::from_mode(0o751),
    )
    .unwrap_or_else(|error| panic!("set fixture mode: {error}"));
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let observation = observation(&mut tools, "shape.rs", 1, 20);
    let admitted = admitted(tools.admit(
        call(
            EDIT_TOOL_NAME,
            json!({
                "path": "./shape.rs",
                "observation": observation,
                "edits": [
                    {"old_text": "alpha", "new_text": "beta"},
                    {"old_text": "old", "new_text": "new"},
                    {"old_text": "#[deprecated]\npub fn legacy() {}\r\n", "new_text": ""}
                ]
            }),
        ),
        &FileCancellation::new(),
    ));

    assert_eq!(
        admitted.capabilities().iter().collect::<Vec<_>>(),
        [ToolCapability::FileRead, ToolCapability::FileWrite]
    );
    let canonical: Value = serde_json::from_str(admitted.canonical_arguments())
        .unwrap_or_else(|error| panic!("canonical edit: {error}"));
    assert_eq!(canonical["path"], "shape.rs");
    assert!(canonical.get("edits").is_none());
    assert_eq!(canonical["splices"].as_array().map(Vec::len), Some(3));
    assert!(matches!(
        admitted.invocation(),
        Some(ToolDetail::Text { source, omitted_bytes: 0 })
            if source == "path: \"shape.rs\"\nexact_replacements: 3"
    ));

    let result = tools.execute(&admitted, &FileCancellation::new());
    let ToolOutcome::Succeeded { output } = result.outcome() else {
        panic!("edit did not succeed");
    };
    let output: Value =
        serde_json::from_str(output).unwrap_or_else(|error| panic!("output JSON: {error}"));
    assert_eq!(output, json!({"path": "shape.rs", "edits_applied": 3}));
    let Some(ToolDetail::Diff { patch }) = result.presentation() else {
        panic!("successful edit must carry its canonical patch");
    };
    assert!(patch.contains("-alpha\n\\ No newline at end of edit\n+beta\n"));
    assert!(patch.contains("-old\n\\ No newline at end of edit\n+new\n"));
    assert!(patch.contains("-#[deprecated]\n-pub fn legacy() {}\r\n"));
    assert_eq!(
        fs::read(workspace.path().join("shape.rs"))
            .unwrap_or_else(|error| panic!("read result: {error}")),
        expected.as_bytes()
    );
    assert_eq!(
        fs::metadata(workspace.path().join("shape.rs"))
            .unwrap_or_else(|error| panic!("result metadata: {error}"))
            .permissions()
            .mode()
            & 0o777,
        0o751
    );
}

/// MUT-1/MUT-2: exactness is scoped to bytes the named observation actually returned.
#[test]
fn admission_enforces_the_observed_window_and_unique_target() {
    let workspace = TestWorkspace::new();
    workspace.write("window", b"target\noutside\ntarget\nabc\n");
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));

    let first_line = observation(&mut tools, "window", 1, 1);
    assert!(matches!(
        tools.admit(
            call(
                EDIT_TOOL_NAME,
                json!({"path":"window", "observation":first_line, "edits":[{"old_text":"target", "new_text":"first"}]})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Admitted(_)
    ));
    assert!(matches!(
        tools.admit(
            call(
                EDIT_TOOL_NAME,
                json!({"path":"window", "observation":first_line, "edits":[{"old_text":"outside", "new_text":"hidden"}]})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Refused { reason: AdmissionRefusal::SourceMismatch, .. }
    ));

    let all = observation(&mut tools, "window", 1, 10);
    assert!(matches!(
        tools.admit(
            call(
                EDIT_TOOL_NAME,
                json!({"path":"window", "observation":all, "edits":[{"old_text":"target", "new_text":"ambiguous"}]})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Refused { reason: AdmissionRefusal::AmbiguousTarget, .. }
    ));
    assert!(matches!(
        tools.admit(
            call(
                EDIT_TOOL_NAME,
                json!({"path":"window", "observation":all, "edits":[
                    {"old_text":"abc", "new_text":"xyz"},
                    {"old_text":"bc", "new_text":"yz"}
                ]})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::ConflictingArguments,
            ..
        }
    ));
    workspace.write("overlap", b"aaa\n");
    let overlap = observation(&mut tools, "overlap", 1, 1);
    assert!(matches!(
        tools.admit(
            call(
                EDIT_TOOL_NAME,
                json!({"path":"overlap", "observation":overlap, "edits":[{"old_text":"aa", "new_text":"b"}]})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::AmbiguousTarget,
            ..
        }
    ));
    assert_eq!(
        fs::read(workspace.path().join("window"))
            .unwrap_or_else(|error| panic!("read unchanged fixture: {error}")),
        b"target\noutside\ntarget\nabc\n"
    );
}

/// MUT-5: create is a separate absence-only capability and a later creator always wins.
#[test]
fn create_is_absence_only_and_never_overwrites() {
    let workspace = TestWorkspace::new();
    fs::create_dir(workspace.path().join("src"))
        .unwrap_or_else(|error| panic!("create parent: {error}"));
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let create = admitted(tools.admit(
        call(
            CREATE_TOOL_NAME,
            json!({"path":"./src/generated.rs", "content":"pub const GENERATED: &str = \"λ\";\n"}),
        ),
        &FileCancellation::new(),
    ));
    assert_eq!(
        create.capabilities().iter().collect::<Vec<_>>(),
        [ToolCapability::FileWrite]
    );
    assert!(matches!(
        create.invocation(),
        Some(ToolDetail::Text { source, omitted_bytes: 0 })
            if source.contains("path: \"src/generated.rs\"")
                && source.contains("content_bytes: 34")
    ));
    fs::write(workspace.path().join("src/generated.rs"), b"operator\n")
        .unwrap_or_else(|error| panic!("concurrent create: {error}"));
    assert_eq!(
        failure_kind(tools.execute(&create, &FileCancellation::new())),
        "create_collision"
    );
    assert_eq!(
        fs::read(workspace.path().join("src/generated.rs"))
            .unwrap_or_else(|error| panic!("read collision: {error}")),
        b"operator\n"
    );
    assert!(matches!(
        tools.admit(
            call(
                CREATE_TOOL_NAME,
                json!({"path":"src/generated.rs", "content":"replace"})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::StalePrecondition,
            ..
        }
    ));

    let fresh = admitted(tools.admit(
        call(
            CREATE_TOOL_NAME,
            json!({"path":"src/fresh.rs", "content":"fresh\n"}),
        ),
        &FileCancellation::new(),
    ));
    let created = tools.execute(&fresh, &FileCancellation::new());
    let ToolOutcome::Succeeded { output } = created.outcome() else {
        panic!("fresh create did not succeed: {created:?}");
    };
    assert!(matches!(
        created.presentation(),
        Some(ToolDetail::Text { source, omitted_bytes: 0 }) if source == output
    ));
    assert_eq!(
        fs::read(workspace.path().join("src/fresh.rs"))
            .unwrap_or_else(|error| panic!("read created file: {error}")),
        b"fresh\n"
    );
}

/// MUT-2/MUT-6: unknown structure, binary content, forged canonical edits, and cancellation fail
/// closed without changing workspace bytes.
#[test]
fn malformed_canonical_and_cancelled_mutations_fail_closed() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"old\n");
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let observation = observation(&mut tools, "file", 1, 10);
    for arguments in [
        json!({"path":"file", "observation":observation, "edits":[{"old_text":"old", "new_text":"new", "surprise":true}]}),
        json!({"path":"file", "observation":observation, "edits":[]}),
    ] {
        assert!(matches!(
            tools.admit(call(EDIT_TOOL_NAME, arguments), &FileCancellation::new()),
            AdmissionOutcome::Refused {
                reason: AdmissionRefusal::InvalidArguments,
                ..
            }
        ));
    }
    assert!(matches!(
        tools.admit(
            call(
                CREATE_TOOL_NAME,
                json!({"path":"binary", "content":"a\u{0}b"})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::InvalidArguments,
            ..
        }
    ));

    let valid = admitted(tools.admit(
        call(
            EDIT_TOOL_NAME,
            json!({"path":"file", "observation":observation, "edits":[{"old_text":"old", "new_text":"new"}]})
        ),
        &FileCancellation::new(),
    ));
    let wrong_capabilities = admitted(
        call(
            EDIT_TOOL_NAME,
            json!({"path":"file", "observation":observation, "edits":[{"old_text":"old", "new_text":"new"}]}),
        )
        .admit(
            valid.definition_id().clone(),
            valid.definition_revision(),
            [ToolCapability::FileRead],
            valid.canonical_arguments().to_owned(),
            valid.detail().to_owned(),
            valid.invocation().cloned(),
        )
        .unwrap_or_else(|error| panic!("capability fixture: {error:?}")),
    );
    assert_eq!(
        failure_kind(tools.execute(&wrong_capabilities, &FileCancellation::new())),
        "definition_mismatch"
    );
    let mut canonical: Value = serde_json::from_str(valid.canonical_arguments())
        .unwrap_or_else(|error| panic!("canonical edit: {error}"));
    canonical["splices"][0]["expected"] = json!("forged");
    let forged = admitted(
        call(
            EDIT_TOOL_NAME,
            json!({"path":"file", "observation":observation, "edits":[{"old_text":"old", "new_text":"new"}]}),
        )
        .admit(
            valid.definition_id().clone(),
            valid.definition_revision(),
            valid.capabilities().iter(),
            canonical.to_string(),
            valid.detail().to_owned(),
            valid.invocation().cloned(),
        )
        .unwrap_or_else(|error| panic!("forged call fixture: {error:?}")),
    );
    assert_eq!(
        failure_kind(tools.execute(&forged, &FileCancellation::new())),
        "invalid_arguments"
    );

    let cancelled = FileCancellation::new();
    cancelled.cancel();
    assert!(matches!(
        tools.admit(
            call(
                EDIT_TOOL_NAME,
                json!({"path":"file", "observation":observation, "edits":[{"old_text":"old", "new_text":"new"}]})
            ),
            &cancelled
        ),
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::Cancelled,
            ..
        }
    ));
    assert_eq!(failure_kind(tools.execute(&valid, &cancelled)), "cancelled");
    assert_eq!(
        fs::read(workspace.path().join("file"))
            .unwrap_or_else(|error| panic!("read unchanged file: {error}")),
        b"old\n"
    );
}
