//! Composition-root ordering and the executable's private search-driver seam.

use std::{
    fs::{self, File},
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

struct TestWorkspace(PathBuf);

impl TestWorkspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "plexmaton-cli-startup-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap_or_else(|error| panic!("create test workspace: {error}"));
        Self(path)
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0)
            .unwrap_or_else(|error| panic!("remove test workspace: {error}"));
    }
}

/// JRN-4: session command discovery needs neither configuration nor terminal ownership.
#[test]
fn help_names_the_explicit_create_and_resume_surface_before_startup() {
    let missing =
        std::env::temp_dir().join(format!("plexmaton-missing-help-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_plexmaton"))
        .arg("--help")
        .env("PLEXMATON_HOME", missing)
        .output()
        .unwrap_or_else(|error| panic!("run executable help: {error}"));

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create <session-id>"));
    assert!(stdout.contains("resume <session-id>"));
    assert!(
        !output
            .stdout
            .windows(8)
            .any(|bytes| bytes == b"\x1b[?1049h")
    );
}

fn ripgrep() -> PathBuf {
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if directory.is_absolute() {
            let candidate = directory.join("rg");
            if candidate.is_file() {
                return fs::canonicalize(&candidate)
                    .unwrap_or_else(|error| panic!("canonicalize ripgrep: {error}"));
            }
        }
    }
    panic!("ripgrep is required for CLI integration tests");
}

/// LIVE-6: invalid configuration fails before the executable writes alternate-screen ownership.
#[test]
fn invalid_configuration_never_takes_over_the_terminal() {
    let missing =
        std::env::temp_dir().join(format!("plexmaton-missing-config-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_plexmaton"))
        .env("PLEXMATON_HOME", missing)
        .output()
        .unwrap_or_else(|error| panic!("run executable: {error}"));

    assert!(!output.status.success());
    assert!(
        !output
            .stdout
            .windows(8)
            .any(|bytes| bytes == b"\x1b[?1049h"),
        "invalid startup entered the alternate screen"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("read provider configuration"),
        "startup did not report the configuration boundary"
    );
}

/// WFS-1 and LIVE-6: the installed executable can become the descriptor-rooted search driver
/// before configuration or terminal ownership, so no uninstalled companion binary is assumed.
#[test]
fn executable_private_driver_reenters_a_pinned_directory_and_execs_ripgrep() {
    let workspace = TestWorkspace::new();
    fs::write(workspace.0.join("visible.txt"), "fixture\n")
        .unwrap_or_else(|error| panic!("write test file: {error}"));
    let output = Command::new(env!("CARGO_BIN_EXE_plexmaton"))
        .arg("--__plexmaton-rg-driver")
        .arg(ripgrep())
        .args([
            "--files",
            "--null",
            "--no-config",
            "--no-require-git",
            "--no-follow",
            "--",
            ".",
        ])
        .stdin(Stdio::from(File::open(&workspace.0).unwrap_or_else(
            |error| panic!("open pinned directory: {error}"),
        )))
        .output()
        .unwrap_or_else(|error| panic!("run executable search driver: {error}"));

    assert!(
        output.status.success(),
        "search driver failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output
            .stdout
            .split(|byte| *byte == 0)
            .any(|path| path == b"./visible.txt"),
        "search driver did not enumerate its descriptor-rooted fixture: {:?}",
        output.stdout
    );
}
