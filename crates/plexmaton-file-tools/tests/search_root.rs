mod support;

use plexmaton_file_tools::{FileCancellation, FileTools, PathError, SearchError, SearchRequest};
use support::{TestWorkspace, rg_executable, search_driver};

/// WFS-1: a symbolic-link target never gives the model authority outside the workspace.
#[cfg(unix)]
#[test]
fn a_symlinked_search_target_is_refused_before_spawn() {
    let workspace = TestWorkspace::new();
    std::os::unix::fs::symlink("/tmp", workspace.path().join("escape"))
        .unwrap_or_else(|error| panic!("symlink fixture: {error}"));
    let tools = FileTools::open(workspace.path(), rg_executable(), search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    let request = SearchRequest::new("anything".to_owned(), Some("escape".to_owned()), None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    assert_eq!(
        tools.search(&request, &FileCancellation::new()),
        Err(SearchError::Path(PathError::Symlink))
    );
}

/// WFS-1: later pathname replacement cannot redirect the already-open workspace root.
#[cfg(unix)]
#[test]
fn replacing_the_root_path_does_not_redirect_search() {
    let workspace = TestWorkspace::new();
    workspace.write("root/file", b"pinned\n");
    let root = workspace.path().join("root");
    let tools = FileTools::open(&root, rg_executable(), search_driver())
        .unwrap_or_else(|error| panic!("open tools: {error}"));
    std::fs::rename(&root, workspace.path().join("original"))
        .unwrap_or_else(|error| panic!("rename root: {error}"));
    workspace.write("root/file", b"replacement\n");
    let request = SearchRequest::new("pinned".to_owned(), None, None, None)
        .unwrap_or_else(|error| panic!("request: {error}"));

    let result = tools
        .search(&request, &FileCancellation::new())
        .unwrap_or_else(|error| panic!("search pinned root: {error}"));
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].path, "file");
    assert_eq!(result.matches[0].preview, "pinned");
}
