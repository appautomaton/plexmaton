mod support;

use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, MAX_ADMITTED_ARGUMENT_BYTES,
    MAX_REQUESTED_TOOL_ARGUMENT_BYTES,
};
use plexmaton_file_tools::{
    CREATE_TOOL_NAME, EDIT_TOOL_NAME, FileCancellation, FileTools, ReadRequest,
};
use serde_json::json;
use support::{TestWorkspace, admission_request};

fn observation(tools: &mut FileTools, path: &str, limit: u16) -> String {
    tools
        .read(
            &ReadRequest::new(path.to_owned(), Some(1), Some(limit))
                .unwrap_or_else(|error| panic!("read request: {error}")),
            &FileCancellation::new(),
        )
        .unwrap_or_else(|error| panic!("read fixture: {error}"))
        .observation
        .as_token()
}

fn is_invalid(outcome: AdmissionOutcome) -> bool {
    matches!(
        outcome,
        AdmissionOutcome::Refused {
            reason: AdmissionRefusal::InvalidArguments,
            ..
        }
    )
}

/// MUT-6: edit-count, aggregate argument, source, and result byte bounds accept their exact edge
/// and refuse the first value beyond it.
#[test]
fn mutation_bounds_hold_at_their_exact_edges() {
    const ARGUMENT_BYTES: usize = 48 * 1024;
    const SOURCE_BYTES: usize = 8 * 1024 * 1024;

    let workspace = TestWorkspace::new();
    let tokens = (0..17)
        .map(|index| format!("token-{index:02}"))
        .collect::<Vec<_>>();
    workspace.write("edits", format!("{}\n", tokens.join("\n")).as_bytes());
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    assert!(matches!(
        tools.admit(
            admission_request(
                CREATE_TOOL_NAME,
                json!({"path":"create-at-limit", "content":"x".repeat(ARGUMENT_BYTES)})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Admitted(_)
    ));
    assert!(is_invalid(tools.admit(
        admission_request(
            CREATE_TOOL_NAME,
            json!({"path":"create-over-limit", "content":"x".repeat(ARGUMENT_BYTES + 1)})
        ),
        &FileCancellation::new()
    )));
    let unicode_at_limit = "🦀".repeat(ARGUMENT_BYTES / 4);
    assert_eq!(unicode_at_limit.len(), ARGUMENT_BYTES);
    assert!(matches!(
        tools.admit(
            admission_request(
                CREATE_TOOL_NAME,
                json!({"path":"unicode-at-limit", "content":unicode_at_limit})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Admitted(_)
    ));
    let unicode_over_limit = "🦀".repeat(ARGUMENT_BYTES / 4 + 1);
    assert_eq!(unicode_over_limit.chars().count(), ARGUMENT_BYTES / 4 + 1);
    assert!(is_invalid(tools.admit(
        admission_request(
            CREATE_TOOL_NAME,
            json!({"path":"unicode-over-limit", "content":unicode_over_limit})
        ),
        &FileCancellation::new()
    )));
    let observed = observation(&mut tools, "edits", 20);
    let edits = tokens
        .iter()
        .map(|token| json!({"old_text":token, "new_text":token.to_uppercase()}))
        .collect::<Vec<_>>();
    assert!(matches!(
        tools.admit(
            admission_request(
                EDIT_TOOL_NAME,
                json!({"path":"edits", "observation":observed, "edits":&edits[..16]})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Admitted(_)
    ));
    assert!(is_invalid(tools.admit(
        admission_request(
            EDIT_TOOL_NAME,
            json!({"path":"edits", "observation":observed, "edits":edits})
        ),
        &FileCancellation::new()
    )));

    let old = format!("{}\n{}", "a".repeat(12_000), "b".repeat(12_575));
    let replacement = format!("{}\n{}", "c".repeat(12_000), "d".repeat(12_575));
    assert_eq!(old.len() + replacement.len(), ARGUMENT_BYTES);
    workspace.write("arguments", old.as_bytes());
    let observed = observation(&mut tools, "arguments", 3);
    assert!(matches!(
        tools.admit(
            admission_request(
                EDIT_TOOL_NAME,
                json!({"path":"arguments", "observation":observed, "edits":[{"old_text":old, "new_text":replacement}]})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Admitted(_)
    ));
    assert!(is_invalid(tools.admit(
        admission_request(
            EDIT_TOOL_NAME,
            json!({"path":"arguments", "observation":observed, "edits":[{"old_text":old, "new_text":format!("{replacement}x")}]})
        ),
        &FileCancellation::new()
    )));

    let mut at_limit = b"needle\n".to_vec();
    at_limit.resize(SOURCE_BYTES, b'a');
    workspace.write("source-limit", &at_limit);
    let observed = observation(&mut tools, "source-limit", 1);
    assert!(matches!(
        tools.admit(
            admission_request(
                EDIT_TOOL_NAME,
                json!({"path":"source-limit", "observation":observed, "edits":[{"old_text":"needle", "new_text":"NEEDLE"}]})
            ),
            &FileCancellation::new()
        ),
        AdmissionOutcome::Admitted(_)
    ));
    assert!(is_invalid(tools.admit(
        admission_request(
            EDIT_TOOL_NAME,
            json!({"path":"source-limit", "observation":observed, "edits":[{"old_text":"needle", "new_text":"NEEDLES"}]})
        ),
        &FileCancellation::new()
    )));

    at_limit.push(b'a');
    workspace.write("source-over", &at_limit);
    let observed = observation(&mut tools, "source-over", 1);
    assert!(is_invalid(tools.admit(
        admission_request(
            EDIT_TOOL_NAME,
            json!({"path":"source-over", "observation":observed, "edits":[{"old_text":"needle", "new_text":"NEEDLE"}]})
        ),
        &FileCancellation::new()
    )));
}

/// MUT-6: a provider-valid escaped edit remains admissible after trusted canonicalization adds
/// splice offsets and field names.
#[test]
fn escaped_edit_within_raw_and_mutation_bounds_survives_canonicalization() {
    let workspace = TestWorkspace::new();
    let old_text = "\"".repeat(16_350);
    let new_text = "\\".repeat(16_351);
    assert!(old_text.len() + new_text.len() < 48 * 1024);
    workspace.write("escaped", old_text.as_bytes());
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let observed = observation(&mut tools, "escaped", 1);
    let arguments = json!({
        "path": "escaped",
        "observation": observed,
        "edits": [{"old_text": old_text, "new_text": new_text}],
    });
    let raw = arguments.to_string();
    assert_eq!(raw.len(), 65_497);
    assert!(raw.len() <= MAX_REQUESTED_TOOL_ARGUMENT_BYTES);

    let outcome = tools.admit(
        admission_request(EDIT_TOOL_NAME, arguments),
        &FileCancellation::new(),
    );
    let AdmissionOutcome::Admitted(call) = outcome else {
        panic!("canonical expansion refused a provider-valid edit: {outcome:?}");
    };
    assert_eq!(call.canonical_arguments().len(), 65_543);
    assert!(call.canonical_arguments().len() <= MAX_ADMITTED_ARGUMENT_BYTES);
}
