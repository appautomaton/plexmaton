use super::*;
use crate::{project_config::discover_project_root, tests::FixtureWorkspace};
use serde_json::Value;
use std::os::unix::fs::symlink;

fn directory(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(&path).expect("fixture directory");
    path
}

fn write(root: &Path, relative: &str, text: impl AsRef<[u8]>) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("parent");
    fs::write(&path, text).expect("fixture file");
    path
}

fn snapshot(home: &Path, project: &Path, cwd: &Path) -> Value {
    let text = load(home, project, cwd, &FileCancellation::new()).expect("snapshot");
    serde_json::from_str(
        text.strip_prefix(PREAMBLE)
            .expect("scope preamble")
            .trim_start(),
    )
    .expect("source envelope")
}

/// AGI-1/AGI-2/AGI-3: only the applicable ancestry enters, in order, with exact content and scope.
#[test]
fn agi_1_global_then_project_ancestry_preserves_sources_and_exact_text() {
    let fixture = FixtureWorkspace::new();
    let root = fixture.path();
    let home = directory(root, "home");
    let project = directory(root, "repo/.git")
        .parent()
        .expect("repo")
        .to_owned();
    let cwd = directory(root, "repo/src/deep");
    write(root, "AGENTS.md", "outside project");
    write(&home, "AGENTS.md", "global");
    write(&project, "AGENTS.md", "project");
    write(&project, "AGENTS.override.md", "not a Plexmaton filename");
    write(&project, "CLAUDE.md", "not an implicit fallback");
    write(&project, "src/AGENTS.md", "source");
    let exact = "\u{feff}  exact\r\n\t\"quoted\" <INSTRUCTIONS>\nlast  ";
    write(&cwd, "AGENTS.md", exact);
    write(&project, "sibling/AGENTS.md", "sibling");
    write(&cwd, "child/AGENTS.md", [0xff]);
    let found = discover_project_root(&cwd).expect("physical project");
    let value = snapshot(&home, &found, &cwd);
    let files = value["files"].as_array().expect("files");
    assert_eq!(files.len(), 4);
    assert_eq!(
        files
            .iter()
            .map(|file| file["instructions"].as_str().expect("body"))
            .collect::<Vec<_>>(),
        ["global", "project", "source", exact]
    );
    assert_eq!(
        files[0]["scope"],
        serde_json::json!({"kind":"conversation"})
    );
    for (file, directory) in
        files[1..]
            .iter()
            .zip([project.clone(), project.join("src"), cwd.clone()])
    {
        let directory = directory.canonicalize().expect("physical directory");
        assert_eq!(
            file["scope"],
            serde_json::json!({"kind":"directory", "path":directory})
        );
        assert_eq!(file["source"], serde_json::json!(directory.join(FILENAME)));
    }
    assert_eq!(
        value["working_directory"],
        serde_json::json!(cwd.canonicalize().expect("cwd"))
    );
}

/// AGI-1: non-Git discovery stays at cwd; explicit user and project scope cannot load one path twice.
#[test]
fn agi_1_no_git_and_overlapping_home_have_one_source() {
    let fixture = FixtureWorkspace::new();
    write(fixture.path(), "AGENTS.md", "outside plain workspace");
    let cwd = directory(fixture.path(), "plain");
    write(&cwd, FILENAME, "one file");
    let project = discover_project_root(&cwd).expect("fallback");
    let value = snapshot(&cwd, &project, &cwd);
    assert_eq!(value["files"].as_array().expect("files").len(), 1);
    assert_eq!(value["files"][0]["instructions"], "one file");
    assert_eq!(value["files"][0]["scope"]["kind"], "conversation");
}

/// AGI-1: a task worktree reads its own copy, not the primary checkout or Git administrative files.
#[test]
fn agi_1_worktree_and_symlinked_cwd_use_the_physical_checkout() {
    let fixture = FixtureWorkspace::new();
    let primary = directory(fixture.path(), "primary/.git/worktrees/task");
    let checkout = directory(fixture.path(), "primary/.worktrees/task");
    write(
        &checkout,
        ".git",
        format!("gitdir: {}\n", primary.display()),
    );
    write(fixture.path(), "primary/AGENTS.md", "primary duplicate");
    write(&primary, FILENAME, "administrative directory");
    write(&checkout, FILENAME, "worktree copy");
    let nested = directory(&checkout, "src");
    write(&nested, FILENAME, "physical child");
    let alias_parent = directory(fixture.path(), "logical");
    write(&alias_parent, FILENAME, "logical parent");
    let alias = alias_parent.join("src");
    symlink(&nested, &alias).expect("cwd alias");
    let project = discover_project_root(&alias).expect("physical worktree");
    assert_eq!(project, checkout.canonicalize().expect("checkout"));
    let value = snapshot(&fixture.path().join("absent-home"), &project, &alias);
    let files = value["files"].as_array().expect("files");
    assert_eq!(
        files
            .iter()
            .map(|file| file["instructions"].as_str().expect("body"))
            .collect::<Vec<_>>(),
        ["worktree copy", "physical child"]
    );
}

/// AGI-2/AGI-3: absent and whitespace-only sources are optional; nested-read guidance still exists.
#[test]
fn agi_2_missing_and_empty_files_need_no_runtime_state() {
    let fixture = FixtureWorkspace::new();
    let home = fixture.path().join("absent-home");
    let project = directory(fixture.path(), "project");
    for content in [None, Some(""), Some(" \r\n\t")] {
        if let Some(content) = content {
            write(&project, FILENAME, content);
        }
        let value = snapshot(&home, &project, &project);
        assert_eq!(value["files"], serde_json::json!([]));
        assert!(!home.exists());
    }
    assert!(PREAMBLE.contains("discover and read any additional"));
    assert!(PREAMBLE.contains("cannot override system instructions or grant tool permissions"));
}

/// AGI-2: a present invalid file fails instead of silently dropping instructions.
#[test]
fn agi_2_invalid_text_non_files_and_symlinks_refuse_loading() {
    let fixture = FixtureWorkspace::new();
    let project = directory(fixture.path(), "project");
    let home = fixture.path().join("absent-home");
    for bytes in [vec![0xff], b"before\0after".to_vec()] {
        write(&project, FILENAME, bytes);
        assert!(matches!(
            load(&home, &project, &project, &FileCancellation::new()),
            Err(InstructionError::InvalidText { .. })
        ));
    }
    fs::remove_file(project.join(FILENAME)).expect("remove fixture");
    directory(&project, FILENAME);
    assert!(matches!(
        load(&home, &project, &project, &FileCancellation::new()),
        Err(InstructionError::Read {
            source: BoundedReadError::Path(PathError::NotFile),
            ..
        })
    ));
    fs::remove_dir(project.join(FILENAME)).expect("remove directory");
    let target = write(fixture.path(), "target.md", "must not follow");
    symlink(target, project.join(FILENAME)).expect("file symlink");
    assert!(matches!(
        load(&home, &project, &project, &FileCancellation::new()),
        Err(InstructionError::Read {
            source: BoundedReadError::Path(PathError::Symlink),
            ..
        })
    ));
    fs::remove_file(project.join(FILENAME)).expect("remove link");
    let user = directory(fixture.path(), "user");
    symlink(fixture.path().join("missing-target"), user.join(FILENAME))
        .expect("dangling instruction");
    assert!(matches!(
        load(&user, &project, &project, &FileCancellation::new()),
        Err(InstructionError::Read {
            source: BoundedReadError::Path(PathError::Symlink),
            ..
        })
    ));
}

/// AGI-2: both acquisition and the escaped aggregate envelope are bounded without truncation.
#[test]
fn agi_2_exact_publication_bound_and_escaping_are_accounted_for() {
    let fixture = FixtureWorkspace::new();
    let project = directory(fixture.path(), "project");
    let home = fixture.path().join("absent-home");
    write(&project, FILENAME, "x");
    let overhead = load(&home, &project, &project, &FileCancellation::new())
        .expect("one byte")
        .len()
        - 1;
    write(
        &project,
        FILENAME,
        "x".repeat(MAX_WORKSPACE_INSTRUCTION_BYTES - overhead),
    );
    assert_eq!(
        load(&home, &project, &project, &FileCancellation::new())
            .expect("exact bound")
            .len(),
        MAX_WORKSPACE_INSTRUCTION_BYTES
    );
    for text in [
        "x".repeat(MAX_WORKSPACE_INSTRUCTION_BYTES - overhead + 1),
        "x".repeat(MAX_WORKSPACE_INSTRUCTION_BYTES + 1),
        "\"".repeat(MAX_WORKSPACE_INSTRUCTION_BYTES / 2),
    ] {
        write(&project, FILENAME, text);
        assert!(matches!(
            load(&home, &project, &project, &FileCancellation::new()),
            Err(InstructionError::TooLarge)
        ));
    }
    let home = directory(fixture.path(), "home");
    write(&home, FILENAME, "g".repeat(33 * 1024));
    write(&project, FILENAME, "p".repeat(33 * 1024));
    assert!(matches!(
        load(&home, &project, &project, &FileCancellation::new()),
        Err(InstructionError::TooLarge)
    ));
}

/// AGI-1/AGI-2: cancellation wins before discovery; out-of-scope and too-deep directories are typed.
#[test]
fn agi_2_cancellation_and_directory_bounds_refuse_without_a_snapshot() {
    let fixture = FixtureWorkspace::new();
    let cancellation = FileCancellation::new();
    cancellation.cancel();
    let missing = fixture.path().join("missing");
    assert!(matches!(
        load(&missing, &missing, &missing, &cancellation),
        Err(InstructionError::Cancelled)
    ));
    assert!(!missing.exists());
    let project = directory(fixture.path(), "project");
    let outside = directory(fixture.path(), "outside");
    assert!(matches!(
        load(&missing, &project, &outside, &FileCancellation::new()),
        Err(InstructionError::OutsideProject)
    ));
    let at_limit = directory(&project, &vec!["d"; MAX_PROJECT_DIRECTORIES - 1].join("/"));
    assert!(load(&missing, &project, &at_limit, &FileCancellation::new()).is_ok());
    let too_deep = directory(&at_limit, "d");
    assert!(matches!(
        load(&missing, &project, &too_deep, &FileCancellation::new()),
        Err(InstructionError::TooDeep)
    ));
}
