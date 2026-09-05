mod support;

use plexmaton_file_tools::{DirectoryListError, FileCancellation, PathError, WorkspaceRoot};
use support::TestWorkspace;

/// WFS-1: every derived directory component is opened descriptor-relative with no-follow.
#[test]
fn derived_directories_refuse_intermediate_and_leaf_symlinks() {
    let workspace = TestWorkspace::new();
    workspace.write("external/skills/escape/SKILL.md", b"outside");
    let root = WorkspaceRoot::open(workspace.path())
        .unwrap_or_else(|error| panic!("pin authority root: {error}"));

    std::os::unix::fs::symlink("external", workspace.path().join(".agents"))
        .unwrap_or_else(|error| panic!("create intermediate symlink: {error}"));
    assert!(matches!(
        root.open_directory(".agents/skills"),
        Err(PathError::Symlink)
    ));
    std::fs::remove_file(workspace.path().join(".agents"))
        .unwrap_or_else(|error| panic!("remove intermediate symlink: {error}"));
    std::fs::create_dir(workspace.path().join(".agents"))
        .unwrap_or_else(|error| panic!("create agents directory: {error}"));
    std::os::unix::fs::symlink(
        "../external/skills",
        workspace.path().join(".agents/skills"),
    )
    .unwrap_or_else(|error| panic!("create leaf symlink: {error}"));
    assert!(matches!(
        root.open_directory(".agents/skills"),
        Err(PathError::Symlink)
    ));
}

/// WFS-1: listing and later reads stay attached to the original derived directory descriptor.
#[test]
fn derived_directory_listing_survives_path_replacement_without_redirecting_reads() {
    let workspace = TestWorkspace::new();
    workspace.write("skills/original/SKILL.md", b"original");
    let authority = WorkspaceRoot::open(workspace.path())
        .unwrap_or_else(|error| panic!("pin authority root: {error}"));
    let skills = authority
        .open_directory("skills")
        .unwrap_or_else(|error| panic!("pin derived root: {error}"));
    std::fs::rename(
        workspace.path().join("skills"),
        workspace.path().join("old-skills"),
    )
    .unwrap_or_else(|error| panic!("rename derived root: {error}"));
    workspace.write("skills/replacement/SKILL.md", b"replacement");

    assert_eq!(
        skills
            .list_names(8, &FileCancellation::new())
            .unwrap_or_else(|error| panic!("list pinned root: {error}")),
        [std::ffi::OsString::from("original")]
    );
    let read = skills
        .read_prefix("original/SKILL.md", 32, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("read pinned child: {error}"));
    assert_eq!(read.bytes, b"original");
    assert!(matches!(
        skills.read_prefix("replacement/SKILL.md", 32, &FileCancellation::new()),
        Err(plexmaton_file_tools::BoundedReadError::Path(
            PathError::NotFound
        ))
    ));
}

/// WFS-1/WFS-6: directory acquisition stops at its entry bound and honors owner cancellation.
#[test]
fn directory_listing_is_bounded_and_cancellable() {
    let workspace = TestWorkspace::new();
    workspace.write("skills/one/SKILL.md", b"one");
    let authority = WorkspaceRoot::open(workspace.path())
        .unwrap_or_else(|error| panic!("pin authority root: {error}"));
    let skills = authority
        .open_directory("skills")
        .unwrap_or_else(|error| panic!("pin derived root: {error}"));

    assert_eq!(
        skills.list_names(0, &FileCancellation::new()),
        Err(DirectoryListError::LimitExceeded { limit: 0 })
    );
    let cancellation = FileCancellation::new();
    cancellation.cancel();
    assert_eq!(
        skills.list_names(8, &cancellation),
        Err(DirectoryListError::Cancelled)
    );
}
