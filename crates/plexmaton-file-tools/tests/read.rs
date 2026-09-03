mod support;

use std::fs::OpenOptions;

use plexmaton_file_tools::{
    FileCancellation, FileTools, PathError, ReadCompletion, ReadError, ReadRequest,
};
use support::TestWorkspace;

/// WFS-2: content is exact and a bounded first window does not inspect a sparse giant tail.
#[test]
fn read_windows_preserve_exact_bytes_without_scanning_the_tail() {
    let workspace = TestWorkspace::new();
    workspace.write("src/file.txt", b"\xef\xbb\xbffirst\r\nsecond\nthird");
    let file = OpenOptions::new()
        .write(true)
        .open(workspace.path().join("src/file.txt"))
        .unwrap_or_else(|error| panic!("open sparse fixture: {error}"));
    file.set_len(100 * 1024 * 1024)
        .unwrap_or_else(|error| panic!("extend sparse fixture: {error}"));
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));

    let result = tools
        .read(
            &ReadRequest::new("src/file.txt".to_owned(), None, Some(2))
                .unwrap_or_else(|error| panic!("request: {error}")),
            &FileCancellation::new(),
        )
        .unwrap_or_else(|error| panic!("bounded read: {error}"));

    assert_eq!(result.content, "\u{feff}first\r\nsecond\n");
    assert_eq!(result.completion, ReadCompletion::LineLimit);
    assert_eq!(result.next_offset, Some(3));
    assert!(result.bytes_examined < 64 * 1024);
}

/// WFS-1: path syntax and descriptor-relative no-follow opening reject every escape route.
#[test]
fn absolute_parent_and_symlink_paths_never_open() {
    let workspace = TestWorkspace::new();
    workspace.write("inside.txt", b"inside\n");
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    assert_eq!(
        ReadRequest::new("../outside".to_owned(), None, None),
        Err(ReadError::Path(PathError::NotRelative))
    );

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/passwd", workspace.path().join("escape"))
            .unwrap_or_else(|error| panic!("symlink fixture: {error}"));
        let result = tools.read(
            &ReadRequest::new("escape".to_owned(), None, None)
                .unwrap_or_else(|error| panic!("request: {error}")),
            &FileCancellation::new(),
        );
        assert_eq!(result, Err(ReadError::Path(PathError::Symlink)));
    }
}

#[cfg(unix)]
#[test]
fn replacing_the_root_path_does_not_redirect_a_read() {
    let workspace = TestWorkspace::new();
    workspace.write("root/file", b"pinned\n");
    let root = workspace.path().join("root");
    let mut tools = FileTools::open(&root, "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    std::fs::rename(&root, workspace.path().join("original"))
        .unwrap_or_else(|error| panic!("rename root: {error}"));
    workspace.write("root/file", b"replacement\n");
    let request = ReadRequest::new("file".to_owned(), None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    let result = tools
        .read(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("read pinned root: {error}"));

    assert_eq!(result.content, "pinned\n");
}

/// WFS-2: binary, malformed UTF-8 and a line larger than the local cap are distinct failures.
#[test]
fn invalid_binary_and_giant_lines_are_typed() {
    let workspace = TestWorkspace::new();
    workspace.write("binary", b"one\0two\n");
    workspace.write("utf8", b"one\xfftwo\n");
    workspace.write("giant", &vec![b'x'; 20 * 1024]);
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));

    for (path, expected) in [
        ("binary", ReadError::Binary { line: 1 }),
        ("utf8", ReadError::InvalidUtf8 { line: 1 }),
        (
            "giant",
            ReadError::LineTooLong {
                line: 1,
                limit: 16 * 1024,
            },
        ),
    ] {
        let request = ReadRequest::new(path.to_owned(), None, None)
            .unwrap_or_else(|error| panic!("request: {error}"));
        assert_eq!(
            tools.read(&request, &FileCancellation::new()),
            Err(expected)
        );
    }
}

/// WFS-3: the model sees only an opaque token while session state retains path/version authority.
#[test]
fn successful_reads_issue_bounded_observations() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"value\n");
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let first = tools
        .read(
            &ReadRequest::new("file".to_owned(), None, None)
                .unwrap_or_else(|error| panic!("request: {error}")),
            &FileCancellation::new(),
        )
        .unwrap_or_else(|error| panic!("read: {error}"));

    assert!(first.observation.as_token().starts_with("obs-"));
    assert_eq!(
        tools
            .observation(first.observation)
            .map(plexmaton_file_tools::ObservedFile::path),
        Some("file")
    );
    let mut last = first.observation;
    for _ in 0..1024 {
        last = tools
            .read(
                &ReadRequest::new("file".to_owned(), None, None)
                    .unwrap_or_else(|error| panic!("request: {error}")),
                &FileCancellation::new(),
            )
            .unwrap_or_else(|error| panic!("read: {error}"))
            .observation;
    }
    assert!(tools.observation(first.observation).is_none());
    assert!(tools.observation(last).is_some());
}

/// WFS-6: observing cancellation before a read publishes no observation.
#[test]
fn a_cancelled_read_publishes_no_observation() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"value\n");
    let mut tools = FileTools::open(workspace.path(), "/bin/false", "/bin/false")
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let cancellation = FileCancellation::new();
    cancellation.cancel();
    let request = ReadRequest::new("file".to_owned(), None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    assert_eq!(
        tools.read(&request, &cancellation),
        Err(ReadError::Cancelled)
    );
    let result = tools
        .read(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("uncancelled read: {error}"));
    assert_eq!(result.observation.as_token(), "obs-0000000000000001");
}
