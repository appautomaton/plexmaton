mod support;

use std::time::Duration;

use plexmaton_file_tools::{
    FileCancellation, FileTools, MAX_SEARCH_FILES, MAX_SEARCH_RESULT_BYTES, PathError,
    SearchCompletion, SearchError, SearchRequest, SearchRunner, WorkspaceRoot,
};
use support::{TestWorkspace, rg_executable, search_driver};

/// WFS-4: reaching the caller's cap stops acquisition and asks the model to refine its query.
#[test]
fn match_overflow_stops_ripgrep_without_an_unusable_continuation() {
    let workspace = TestWorkspace::new();
    let lines = (0..1000)
        .map(|index| format!("needle {index}\n"))
        .collect::<String>();
    workspace.write("many.txt", lines.as_bytes());
    let tools = FileTools::open(workspace.path(), rg_executable(), search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("needle".to_owned(), None, None, Some(5))
        .unwrap_or_else(|error| panic!("request: {error}"));

    let result = tools
        .search(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("search: {error}"));

    assert_eq!(result.matches.len(), 5);
    assert_eq!(result.completion, SearchCompletion::MatchLimit);
    assert!(result.matches_seen <= 6);
    assert!(result.transport_bytes <= 1024 * 1024);
}

/// WFS-4: ordinary ignore and binary behavior is retained without admitting hidden file access.
#[test]
fn ignored_and_binary_files_do_not_enter_results() {
    let workspace = TestWorkspace::new();
    workspace.write(".gitignore", b"ignored.txt\n");
    workspace.write("ignored.txt", b"needle\n");
    workspace.write("binary", b"needle\0binary\n");
    workspace.write("visible.txt", b"needle visible\n");
    let tools = FileTools::open(workspace.path(), rg_executable(), search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("needle".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    let result = tools
        .search(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("search: {error}"));

    assert_eq!(result.completion, SearchCompletion::Complete);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].path, "visible.txt");
}

#[cfg(unix)]
#[test]
fn a_cancelled_search_joins_its_exact_child_and_reader_tasks() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"value\n");
    let marker = workspace.path().join("started");
    let executable = workspace.executable(
        "fake-rg",
        &format!(
            "#!/bin/sh\n: > '{}'\nexec /bin/sleep 30\n",
            marker.display()
        ),
    );
    let tools = FileTools::open(workspace.path(), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let cancellation = FileCancellation::new();
    let canceller = cancellation.clone();
    let wait_marker = marker.clone();
    let cancellation_task = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !wait_marker.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "search did not publish readiness"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        canceller.cancel();
    });
    let request = SearchRequest::new("value".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    assert_eq!(
        tools.search(&request, &cancellation),
        Err(SearchError::Cancelled)
    );
    cancellation_task
        .join()
        .unwrap_or_else(|_| panic!("cancellation task panicked"));
}

#[cfg(unix)]
#[test]
fn stdout_eof_does_not_disable_the_process_deadline() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"value\n");
    let executable = workspace.executable("fake-rg", "#!/bin/sh\nexec 1>&-\nexec /bin/sleep 30\n");
    let runner =
        SearchRunner::new(executable, search_driver()).with_timeout(Duration::from_millis(20));
    let request = SearchRequest::new("value".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));
    let root =
        WorkspaceRoot::open(workspace.path()).unwrap_or_else(|error| panic!("open root: {error}"));

    assert_eq!(
        runner.search(&root, &request, &FileCancellation::new()),
        Err(SearchError::TimedOut)
    );
}

#[cfg(unix)]
#[test]
fn pattern_and_glob_are_distinct_argv_while_path_is_pinned() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"value\n");
    let discovery_arguments = workspace.path().join("discovery-arguments");
    let search_arguments = workspace.path().join("search-arguments");
    let executable = workspace.executable(
        "fake-rg",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"--files\" ]; then\n  printf '%s\\n' \"$@\" > '{}'\n  printf 'file\\0'\nelse\n  printf '%s\\n' \"$@\" > '{}'\nfi\n",
            discovery_arguments.display(),
            search_arguments.display(),
        ),
    );
    let tools = FileTools::open(workspace.path(), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new(
        "needle; touch escaped".to_owned(),
        None,
        Some("*.rs".to_owned()),
        None,
    )
    .unwrap_or_else(|error| panic!("request: {error}"));

    let result = tools
        .search(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("search: {error}"));
    let discovery_arguments = std::fs::read_to_string(discovery_arguments)
        .unwrap_or_else(|error| panic!("read discovery arguments: {error}"));
    let search_arguments = std::fs::read_to_string(search_arguments)
        .unwrap_or_else(|error| panic!("read search arguments: {error}"));

    assert_eq!(result.completion, SearchCompletion::Complete);
    assert!(discovery_arguments.contains("--no-config\n"));
    assert!(discovery_arguments.contains("--no-follow\n"));
    assert!(discovery_arguments.contains("--glob\n*.rs\n--\n.\n"));
    assert!(search_arguments.contains("--\nneedle; touch escaped\n-\n"));
    assert!(!workspace.path().join("escaped").exists());
}

#[cfg(unix)]
#[test]
fn an_explicit_file_target_swap_never_returns_outside_bytes() {
    let workspace = TestWorkspace::new();
    workspace.write("root/file", b"inside\n");
    workspace.write("outside", b"secret\n");
    let target = workspace.path().join("root/file");
    let outside = workspace.path().join("outside");
    let executable = workspace.executable(
        "fake-rg",
        &format!(
            "#!/bin/sh\nrm -f '{target}'\nln -s '{outside}' '{target}'\nIFS= read -r line\nprintf '{{\"type\":\"match\",\"data\":{{\"path\":{{\"text\":\"stdin\"}},\"lines\":{{\"text\":\"%s\\\\n\"}},\"line_number\":1}}}}\\n' \"$line\"\n",
            target = target.display(),
            outside = outside.display(),
        ),
    );
    let tools = FileTools::open(workspace.path().join("root"), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("anything".to_owned(), Some("file".to_owned()), None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    assert_eq!(
        tools.search(&request, &FileCancellation::new()),
        Err(SearchError::ChangedDuringSearch)
    );
}

#[cfg(unix)]
#[test]
fn a_file_change_during_content_search_discards_the_result() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"before\n");
    let target = workspace.path().join("file");
    let executable = workspace.executable(
        "fake-rg",
        &format!(
            "#!/bin/sh\nprintf '{{\"type\":\"match\",\"data\":{{\"path\":{{\"text\":\"stdin\"}},\"lines\":{{\"text\":\"before\\\\n\"}},\"line_number\":1}}}}\\n'\nprintf 'after\\n' >> '{}'\n",
            target.display()
        ),
    );
    let tools = FileTools::open(workspace.path(), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("before".to_owned(), Some("file".to_owned()), None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    assert_eq!(
        tools.search(&request, &FileCancellation::new()),
        Err(SearchError::ChangedDuringSearch)
    );
}

#[cfg(unix)]
#[test]
fn a_discovered_descendant_is_reopened_no_follow_before_content_search() {
    let workspace = TestWorkspace::new();
    workspace.write("root/file", b"inside\n");
    workspace.write("outside", b"secret\n");
    let target = workspace.path().join("root/file");
    let outside = workspace.path().join("outside");
    let content_started = workspace.path().join("content-started");
    let executable = workspace.executable(
        "fake-rg",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"--files\" ]; then\n  printf 'file\\0'\n  rm -f '{target}'\n  ln -s '{outside}' '{target}'\n  exit\nfi\n: > '{content_started}'\n",
            target = target.display(),
            outside = outside.display(),
            content_started = content_started.display(),
        ),
    );
    let tools = FileTools::open(workspace.path().join("root"), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("secret".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    assert_eq!(
        tools.search(&request, &FileCancellation::new()),
        Err(SearchError::Path(PathError::Symlink))
    );
    assert!(!content_started.exists());
}

#[cfg(unix)]
#[test]
fn relative_search_executables_are_refused_before_spawn() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"value\n");
    let tools = FileTools::open(workspace.path(), "rg", search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("value".to_owned(), Some("file".to_owned()), None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    assert_eq!(
        tools.search(&request, &FileCancellation::new()),
        Err(SearchError::UntrustedExecutable)
    );
}

#[cfg(unix)]
#[test]
fn a_file_target_with_a_glob_is_refused_before_spawn() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"inside\n");
    let marker = workspace.path().join("spawned");
    let executable = workspace.executable(
        "fake-rg",
        &format!("#!/bin/sh\n: > '{}'\n", marker.display()),
    );
    let tools = FileTools::open(workspace.path(), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new(
        "inside".to_owned(),
        Some("file".to_owned()),
        Some("*.rs".to_owned()),
        None,
    )
    .unwrap_or_else(|error| panic!("request: {error}"));

    assert_eq!(
        tools.search(&request, &FileCancellation::new()),
        Err(SearchError::InvalidArguments)
    );
    assert!(!marker.exists());
}

#[cfg(unix)]
#[test]
fn a_giant_rg_record_is_typed_and_never_retained() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"value\n");
    let executable = workspace.executable(
        "fake-rg",
        "#!/bin/sh\nif [ \"$1\" = \"--files\" ]; then printf 'file\\0'; exit; fi\nprintf '{\"type\":\"match\",\"padding\":\"'\nhead -c 70000 /dev/zero | tr '\\0' x\nprintf '\"}\\n'\n",
    );
    let tools = FileTools::open(workspace.path(), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("value".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    assert_eq!(
        tools.search(&request, &FileCancellation::new()),
        Err(SearchError::RecordTooLarge { limit: 64 * 1024 })
    );
}

#[cfg(unix)]
#[test]
fn retained_match_bytes_stop_independently_of_transport() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"value\n");
    let preview = "x".repeat(2048);
    let record = format!(
        "{{\"type\":\"match\",\"data\":{{\"path\":{{\"text\":\"file\"}},\"lines\":{{\"text\":\"{preview}\"}},\"line_number\":1}}}}"
    );
    let mut script =
        String::from("#!/bin/sh\nif [ \"$1\" = \"--files\" ]; then printf 'file\\0'; exit; fi\n");
    for _ in 0..100 {
        script.push_str("printf '%s\\n' '");
        script.push_str(&record);
        script.push_str("'\n");
    }
    let executable = workspace.executable("fake-rg", &script);
    let tools = FileTools::open(workspace.path(), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("value".to_owned(), None, None, Some(500))
        .unwrap_or_else(|error| panic!("request: {error}"));

    let result = tools
        .search(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("search: {error}"));

    assert_eq!(result.completion, SearchCompletion::RetainedByteLimit);
    assert!(
        result
            .matches
            .iter()
            .map(|found| found.path.len() + found.preview.len())
            .sum::<usize>()
            <= MAX_SEARCH_RESULT_BYTES
    );
    assert!(result.transport_bytes < 1024 * 1024);
}

#[cfg(unix)]
#[test]
fn candidate_count_is_bounded_before_unbounded_content_work_starts() {
    let workspace = TestWorkspace::new();
    workspace.write("binary", b"value\0binary\n");
    let executable = workspace.executable(
        "fake-rg",
        "#!/bin/sh\nif [ \"$1\" != \"--files\" ]; then exit 1; fi\ni=0\nwhile [ \"$i\" -lt 512 ]; do printf 'binary\\0'; i=$((i + 1)); done\n",
    );
    let tools = FileTools::open(workspace.path(), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("value".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    let result = tools
        .search(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("bounded candidate search: {error}"));

    assert_eq!(result.completion, SearchCompletion::FileLimit);
    assert_eq!(MAX_SEARCH_FILES, 512);
    assert!(result.matches.is_empty());
}

#[cfg(unix)]
#[test]
fn transport_bytes_are_one_limit_across_discovery_and_every_file() {
    let workspace = TestWorkspace::new();
    workspace.write("one", b"value\n");
    workspace.write("two", b"value\n");
    let executable = workspace.executable(
        "fake-rg",
        "#!/bin/sh\nif [ \"$1\" = \"--files\" ]; then printf 'one\\0two\\0'; exit; fi\ni=0\nwhile [ \"$i\" -lt 30000 ]; do printf '{\"type\":\"summary\",\"data\":{}}\\n'; i=$((i + 1)); done\n",
    );
    let tools = FileTools::open(workspace.path(), executable, search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("absent".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    let result = tools
        .search(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("bounded transport search: {error}"));

    assert_eq!(result.completion, SearchCompletion::TransportByteLimit);
    assert!(result.transport_bytes <= 1024 * 1024);
    assert!(result.matches.is_empty());
}
