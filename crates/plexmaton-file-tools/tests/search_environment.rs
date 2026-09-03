mod support;

use std::process::Command;

use plexmaton_file_tools::{FileCancellation, FileTools, SearchRequest};
use support::{TestWorkspace, search_driver};

/// WFS-4: neither direct nor descriptor-rooted ripgrep inherits provider credentials.
#[cfg(unix)]
#[test]
fn search_children_do_not_receive_host_credentials() {
    const CHILD_MARKER: &str = "PLEXMATON_SEARCH_ENV_FIXTURE";
    if std::env::var_os(CHILD_MARKER).is_some() {
        let workspace = TestWorkspace::new();
        workspace.write("file", b"needle\n");
        let executable = workspace.executable(
            "fake-rg",
            "#!/bin/sh\n\
             [ -z \"${PLEXMATON_SEARCH_ENV_FIXTURE+x}\" ] || exit 90\n\
             [ -z \"${PLEXMATON_LOCAL_API_KEY+x}\" ] || exit 91\n\
             [ -z \"${OPENAI_API_KEY+x}\" ] || exit 92\n\
             [ \"$NO_COLOR\" = 1 ] || exit 93\n\
             [ \"$TERM\" = dumb ] || exit 94\n\
             if [ \"$1\" = \"--files\" ]; then printf 'file\\0'; exit; fi\n\
             IFS= read -r line || exit 1\n\
             printf '{\"type\":\"match\",\"data\":{\"path\":{\"text\":\"stdin\"},\"lines\":{\"text\":\"needle\\\\n\"},\"line_number\":1}}\\n'\n",
        );
        let tools = FileTools::open(workspace.path(), executable, search_driver())
            .unwrap_or_else(|error| panic!("open tools: {error}"));
        let request = SearchRequest::new("needle".to_owned(), None, None, None)
            .unwrap_or_else(|error| panic!("request: {error}"));

        let result = tools
            .search(&request, &FileCancellation::new())
            .unwrap_or_else(|error| panic!("search with scrubbed environment: {error}"));
        assert_eq!(result.matches.len(), 1);
        return;
    }

    let output = Command::new(
        std::env::current_exe().unwrap_or_else(|error| panic!("locate test binary: {error}")),
    )
    .args([
        "--exact",
        "search_children_do_not_receive_host_credentials",
        "--nocapture",
    ])
    .env(CHILD_MARKER, "child")
    .env("PLEXMATON_LOCAL_API_KEY", "must-not-leak")
    .env("OPENAI_API_KEY", "must-not-leak")
    .output()
    .unwrap_or_else(|error| panic!("spawn isolated fixture: {error}"));

    assert!(
        output.status.success(),
        "isolated fixture failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
