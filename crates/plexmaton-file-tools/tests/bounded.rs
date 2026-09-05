mod support;

use plexmaton_file_tools::{BoundedReadError, FileCancellation, PathError, WorkspaceRoot};
use support::TestWorkspace;

/// WFS-1/WFS-6: the shared bounded seam retains the pinned no-follow and cancellation boundary.
#[test]
fn bounded_reads_are_pinned_no_follow_and_cancelled_before_return() {
    let workspace = TestWorkspace::new();
    workspace.write("root/file", b"pinned");
    let original_root = workspace.path().join("root");
    let root =
        WorkspaceRoot::open(&original_root).unwrap_or_else(|error| panic!("pin root: {error}"));
    std::fs::rename(&original_root, workspace.path().join("original"))
        .unwrap_or_else(|error| panic!("move original root: {error}"));
    workspace.write("root/file", b"replacement");

    let read = root
        .read_prefix("file", 16, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("read pinned file: {error}"));
    assert_eq!(read.bytes, b"pinned");
    assert!(read.complete);

    let cancellation = FileCancellation::new();
    cancellation.cancel();
    assert_eq!(
        root.read_prefix("file", 16, &cancellation),
        Err(BoundedReadError::Cancelled)
    );
    assert_eq!(
        root.read_prefix("../file", 16, &FileCancellation::new()),
        Err(BoundedReadError::Path(PathError::NotRelative))
    );
}

/// WFS-2: the caller can distinguish a complete file from an exact bounded prefix.
#[test]
fn bounded_read_reports_completion_without_retaining_the_probe_byte() {
    let workspace = TestWorkspace::new();
    workspace.write("file", b"12345");
    let root =
        WorkspaceRoot::open(workspace.path()).unwrap_or_else(|error| panic!("pin root: {error}"));

    let prefix = root
        .read_prefix("file", 4, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("read prefix: {error}"));
    assert_eq!(prefix.bytes, b"1234");
    assert!(!prefix.complete);

    let complete = root
        .read_prefix("file", 5, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("read complete file: {error}"));
    assert_eq!(complete.bytes, b"12345");
    assert!(complete.complete);
}
