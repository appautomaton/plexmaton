//! Composition-root ordering that a library test cannot observe.

use std::process::Command;

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
