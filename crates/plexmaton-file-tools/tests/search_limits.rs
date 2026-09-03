mod support;

use plexmaton_file_tools::{
    FileCancellation, FileTools, MAX_SEARCH_FILE_BYTES, SearchCompletion, SearchError,
    SearchRequest,
};
use support::{TestWorkspace, rg_executable, search_driver};

/// WFS-4: a stable file above the per-file budget is visible as a refinement boundary.
#[test]
fn an_oversized_text_file_reports_its_file_byte_limit() {
    let workspace = TestWorkspace::new();
    let oversized = vec![b'x'; usize::try_from(MAX_SEARCH_FILE_BYTES + 1).unwrap_or(usize::MAX)];
    workspace.write("large.txt", &oversized);
    let tools = FileTools::open(workspace.path(), rg_executable(), search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("x".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    let result = tools
        .search(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("search: {error}"));

    assert_eq!(result.completion, SearchCompletion::FileByteLimit);
    assert!(result.matches.is_empty());
}

/// WFS-4: candidate filtering cannot bypass ripgrep's pattern validation.
#[test]
fn an_invalid_pattern_is_rejected_when_every_candidate_is_binary() {
    let workspace = TestWorkspace::new();
    workspace.write("binary", b"value\0binary\n");
    let tools = FileTools::open(workspace.path(), rg_executable(), search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("[".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    assert!(matches!(
        tools.search(&request, &FileCancellation::new()),
        Err(SearchError::ProcessFailed { code: Some(2), .. })
    ));
}

/// WFS-4: an invalid pattern fails before candidate discovery can consume the transport budget.
#[cfg(unix)]
#[test]
fn an_invalid_pattern_fails_before_discovery_can_exhaust_transport() {
    let workspace = TestWorkspace::new();
    let marker = workspace.path().join("discovery-started");
    let ripgrep = rg_executable();
    let executable = workspace.raw_executable(
        "fake-rg",
        &format!(
            "#!/bin/sh\ncase \" $* \" in\n  *\" --files-with-matches \"*) exec '{}' \"$@\";;\nesac\n: > '{}'\npath=\ni=0\nwhile [ \"$i\" -lt 4000 ]; do path=\"${{path}}a\"; i=$((i + 1)); done\ni=0\nwhile [ \"$i\" -lt 300 ]; do printf '%s\\0' \"$path\"; i=$((i + 1)); done\n",
            ripgrep.display(),
            marker.display(),
        ),
    );
    let tools = FileTools::open(workspace.path(), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));

    let valid = SearchRequest::new("valid".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("valid request: {error}"));
    let bounded = tools
        .search(&valid, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("bounded discovery: {error}"));
    assert_eq!(bounded.completion, SearchCompletion::TransportByteLimit);
    assert!(marker.exists());
    std::fs::remove_file(&marker).unwrap_or_else(|error| panic!("clear marker: {error}"));

    let invalid = SearchRequest::new("[".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("invalid request shape: {error}"));
    assert!(matches!(
        tools.search(&invalid, &FileCancellation::new()),
        Err(SearchError::ProcessFailed { code: Some(2), .. })
    ));
    assert!(!marker.exists());
}
