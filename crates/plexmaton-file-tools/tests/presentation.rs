mod support;

use std::fs;

use plexmaton_agent::{AdmissionOutcome, AdmittedToolCall, ToolOutcome};
use plexmaton_core::ToolDetail;
use plexmaton_file_tools::{
    EDIT_TOOL_NAME, FileCancellation, FileTools, MAX_EDIT_PRESENTATION_BYTES, ReadRequest,
};
use serde_json::json;
use support::{TestWorkspace, admission_request as call};

fn admitted(outcome: AdmissionOutcome) -> AdmittedToolCall {
    match outcome {
        AdmissionOutcome::Admitted(call) => call,
        AdmissionOutcome::Refused { reason, .. } => panic!("unexpected refusal: {reason:?}"),
    }
}

fn observation(tools: &mut FileTools, path: &str) -> String {
    let request = ReadRequest::new(path.to_owned(), Some(1), Some(1000))
        .unwrap_or_else(|error| panic!("read request: {error}"));
    tools
        .read(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("read fixture: {error}"))
        .observation
        .as_token()
}

/// MUT-6/ENT-4: the largest valid changed-text aggregate still produces a complete canonical
/// patch under a bound derived only from mutation arguments, path, and edit count.
#[test]
fn maximum_valid_edit_retains_a_complete_bounded_patch() {
    let workspace = TestWorkspace::new();
    let changed_text = |byte: char| {
        let total = 48 * 1024 / 2;
        let lines = 1000;
        let payload = total - lines;
        let base = payload / lines;
        let remainder = payload % lines;
        let mut text = String::with_capacity(total);
        for index in 0..lines {
            text.extend(std::iter::repeat_n(
                byte,
                base + usize::from(index < remainder),
            ));
            text.push('\n');
        }
        text
    };
    let old = changed_text('a');
    let new = changed_text('b');
    assert_eq!(old.len() + new.len(), 48 * 1024);
    workspace.write("maximum.txt", old.as_bytes());
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let observation = observation(&mut tools, "maximum.txt");
    let admitted = admitted(tools.admit(
        call(
            EDIT_TOOL_NAME,
            json!({
                "path": "maximum.txt",
                "observation": observation,
                "edits": [{"old_text": old, "new_text": new}],
            }),
        ),
        &FileCancellation::new(),
    ));

    let result = tools.execute(&admitted, &FileCancellation::new());

    assert!(matches!(result.outcome(), ToolOutcome::Succeeded { .. }));
    let Some(ToolDetail::Diff { patch }) = result.presentation() else {
        panic!("maximum edit patch");
    };
    assert!(patch.len() <= MAX_EDIT_PRESENTATION_BYTES);
    assert!(patch.starts_with("*** Begin Patch\n*** Update File: maximum.txt\n"));
    assert!(patch.ends_with("*** End Patch\n"));
    let removed = patch
        .lines()
        .filter_map(|line| line.strip_prefix('-'))
        .fold(String::new(), |mut text, line| {
            text.push_str(line);
            text.push('\n');
            text
        });
    let added = patch
        .lines()
        .filter_map(|line| line.strip_prefix('+'))
        .fold(String::new(), |mut text, line| {
            text.push_str(line);
            text.push('\n');
            text
        });
    assert_eq!(removed, old);
    assert_eq!(added, new);
    assert_eq!(
        fs::read(workspace.path().join("maximum.txt"))
            .unwrap_or_else(|error| panic!("read maximum edit: {error}")),
        new.as_bytes()
    );
}

/// MUT-3/ENT-4: a stale edit exposes its typed failure without replacing the admitted invocation
/// or overwriting the concurrent writer.
#[test]
fn stale_edit_preserves_the_concurrent_writer() {
    let workspace = TestWorkspace::new();
    workspace.write("counter.rs", b"pub const RETRIES: usize = 2;\n");
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let observation = observation(&mut tools, "counter.rs");
    let admitted = admitted(tools.admit(
        call(
            EDIT_TOOL_NAME,
            json!({"path":"counter.rs", "observation":observation, "edits":[{"old_text":"2", "new_text":"3"}]})
        ),
        &FileCancellation::new(),
    ));
    workspace.write(
        "counter.rs",
        b"// changed by operator\npub const RETRIES: usize = 2;\n",
    );

    let stale = tools.execute(&admitted, &FileCancellation::new());

    assert!(matches!(
        stale.presentation(),
        Some(ToolDetail::Text { source, omitted_bytes: 0 })
            if source.contains("stale_observation")
    ));
    assert!(matches!(
        stale.outcome(),
        ToolOutcome::Failed { message } if message.contains("stale_observation")
    ));
    assert_eq!(
        fs::read(workspace.path().join("counter.rs"))
            .unwrap_or_else(|error| panic!("read external change: {error}")),
        b"// changed by operator\npub const RETRIES: usize = 2;\n"
    );
}
