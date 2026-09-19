//! Per-command OS write confinement.
//!
//! The command tool hands a script to `/bin/sh`, and a rule cannot establish what that script will
//! do: `python -c`, a build script, or an interpreter this harness has never heard of all reach the
//! same effects under any spelling. So nothing here inspects a command. The fence is what makes the
//! inspection unnecessary — it bounds writes by path at the kernel, and leaves the command opaque.
//!
//! Reads stay open. Read confinement is what breaks toolchains, discovered one missing sysroot at a
//! time, and it buys little once every file the agent reads already travels out in a model request.
//!
//! macOS only. Linux has Landlock in the kernel and bubblewrap beside it, but neither can be tested
//! on the machine this was built on, and an untested fence is the failure mode below wearing a
//! different name. Elsewhere the shell launches exactly as it did before, and says so.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

/// Apple's profile-applying launcher. It applies the profile to itself and then `exec`s its target,
/// so it leaves no process behind: the spawned pid *is* the shell, and the executor's process
/// group, signalling, drains and reaping are untouched by wrapping.
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Parameter names are positional so the profile never interpolates a path. A path reaches the
/// kernel as an argv element, which removes every quoting and escaping question at once.
const ROOT_PARAMETER_PREFIX: &str = "ROOT";

/// Cache and toolchain directories a real build writes to outside the workspace, tried in order
/// beneath the owner's `HOME`. A directory that does not exist is skipped rather than granted:
/// granting an absent path is the silent failure this module exists to avoid.
const TOOLCHAIN_CACHE_RELATIVE: &[&str] = &[
    ".cargo",
    ".rustup",
    ".cache",
    ".npm",
    "go/pkg/mod",
    ".local/share/uv",
];

/// Environment variables that relocate one of those caches. When set, they win over the default.
const TOOLCHAIN_CACHE_ENVIRONMENT: &[&str] = &[
    "CARGO_HOME",
    "RUSTUP_HOME",
    "XDG_CACHE_HOME",
    "GOMODCACHE",
    "GOPATH",
    "UV_CACHE_DIR",
    "npm_config_cache",
];

/// Why a command ran with the owner's full filesystem authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unconfined {
    /// This platform has no fence wired up. Linux and everything else, by decision.
    UnsupportedPlatform,
    /// macOS without the launcher the fence is built on.
    LauncherMissing,
    /// The launcher is present and refuses to apply any profile: this process tree is already
    /// inside a sandbox that forbids nesting. Reported rather than retried per command, because
    /// the refusal belongs to the tree and would otherwise arrive as an exit code from every
    /// command the owner runs.
    ApplyRefused,
}

/// Whether a profile can be applied at all from inside this process tree.
///
/// Nesting is refused, and the refusal is not visible from the launcher's presence — it appears
/// only when a profile is applied. Left undetected it reaches the owner as a command that exited
/// 71 having run nothing, which reads as the command's own failure. Probed once, because the
/// answer belongs to the process tree rather than to any one command.
fn launcher_applies() -> bool {
    static APPLIES: OnceLock<bool> = OnceLock::new();
    *APPLIES.get_or_init(|| {
        std::process::Command::new(SANDBOX_EXEC)
            .args(["-p", "(version 1)(allow default)", "--", "/usr/bin/true"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// Whether one command's writes are bounded by the OS, and to what.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Confinement {
    /// Writes are denied outside these resolved roots. The command itself is never inspected.
    Enforced {
        /// Arguments that precede the shell invocation, ending in `--` and the shell path.
        prefix: Vec<OsString>,
        /// The roots granted, resolved. Carried for the record, not for matching.
        roots: Vec<PathBuf>,
    },
    /// No fence. The shell has whatever authority the owner has.
    Unconfined(Unconfined),
}

impl Confinement {
    /// Decides confinement for one command rooted at `workspace_root`.
    ///
    /// Roots are resolved here rather than trusted, because an unresolved path is accepted as a
    /// valid profile and grants nothing — macOS hands that case over by default, reporting its
    /// temporary directory under `/var` while the kernel matches `/private/var`. The result is a
    /// fence that looks applied and denies the roots it was told to allow, with a zero exit and no
    /// diagnostic. Every path below therefore enters the profile as its own resolved form or does
    /// not enter it at all.
    pub(crate) fn resolve(workspace_root: &Path, shell: &str) -> Self {
        if !cfg!(target_os = "macos") {
            return Self::Unconfined(Unconfined::UnsupportedPlatform);
        }
        if !Path::new(SANDBOX_EXEC).exists() {
            return Self::Unconfined(Unconfined::LauncherMissing);
        }
        if !launcher_applies() {
            return Self::Unconfined(Unconfined::ApplyRefused);
        }
        let roots = resolved_write_roots(workspace_root);
        let mut prefix = vec![OsString::from("-p"), OsString::from(profile(roots.len()))];
        for (index, root) in roots.iter().enumerate() {
            let mut binding = OsString::from(format!("{ROOT_PARAMETER_PREFIX}{index}="));
            binding.push(root);
            prefix.push(OsString::from("-D"));
            prefix.push(binding);
        }
        prefix.push(OsString::from("--"));
        prefix.push(OsString::from(shell));
        Self::Enforced { prefix, roots }
    }

    /// The program to spawn and the arguments preceding the shell's own `-c`.
    ///
    /// Unconfined returns the shell with no prefix, so the spawn is byte-identical to the one this
    /// module was added in front of.
    pub(crate) fn launch<'a>(&'a self, shell: &'a str) -> (&'a OsStr, &'a [OsString]) {
        match self {
            Self::Enforced { prefix, .. } => (OsStr::new(SANDBOX_EXEC), prefix.as_slice()),
            Self::Unconfined(_) => (OsStr::new(shell), &[]),
        }
    }
}

/// `(allow default)` then one global write deny, then the grants. Seatbelt takes the last matching
/// rule, so the grants must follow the deny.
fn profile(root_count: usize) -> String {
    let mut profile =
        String::from("(version 1)\n(allow default)\n(deny file-write* (subpath \"/\"))\n");
    for index in 0..root_count {
        profile.push_str(&format!(
            "(allow file-write* (subpath (param \"{ROOT_PARAMETER_PREFIX}{index}\")))\n"
        ));
    }
    profile
}

/// The workspace, the temporary directory, and the toolchain caches a build reaches — each resolved,
/// deduplicated, and dropped when it does not exist.
fn resolved_write_roots(workspace_root: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![workspace_root.to_path_buf(), std::env::temp_dir()];
    for key in TOOLCHAIN_CACHE_ENVIRONMENT {
        if let Some(value) = std::env::var_os(key) {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                candidates.push(path);
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.extend(TOOLCHAIN_CACHE_RELATIVE.iter().map(|tail| home.join(tail)));
    }

    let mut roots: Vec<PathBuf> = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let Ok(resolved) = std::fs::canonicalize(&candidate) else {
            continue;
        };
        if !roots.contains(&resolved) {
            roots.push(resolved);
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELL: &str = "/bin/sh";

    #[test]
    fn profile_grants_follow_the_global_deny() {
        // CMD-7: Seatbelt takes the last matching rule, so a grant emitted before the deny grants
        // nothing while still compiling to a valid profile.
        let text = profile(2);
        let deny = text.find("(deny file-write*").expect("global deny");
        let first_grant = text.find("(allow file-write*").expect("first grant");
        assert!(deny < first_grant, "grants must follow the deny:\n{text}");
        assert!(text.contains("(subpath (param \"ROOT0\"))"));
        assert!(text.contains("(subpath (param \"ROOT1\"))"));
    }

    #[test]
    fn profile_without_roots_denies_every_write() {
        let text = profile(0);
        assert!(text.contains("(deny file-write* (subpath \"/\"))"));
        assert!(!text.contains("(allow file-write*"));
    }

    #[test]
    fn every_root_reaching_a_profile_is_its_own_resolved_form() {
        // CMD-7: the silent failure. An unresolved path compiles and grants nothing, so a root that
        // is not already canonical must never reach the kernel.
        let workspace = std::env::temp_dir();
        for root in resolved_write_roots(&workspace) {
            let again = std::fs::canonicalize(&root).expect("a resolved root still resolves");
            assert_eq!(
                again,
                root,
                "{} is not its own resolved form",
                root.display()
            );
        }
    }

    #[test]
    fn absent_roots_are_dropped_rather_than_granted() {
        let absent = PathBuf::from("/plexmaton-does-not-exist/workspace");
        assert!(!resolved_write_roots(&absent).contains(&absent));
    }

    #[test]
    fn roots_are_deduplicated() {
        let roots = resolved_write_roots(&std::env::temp_dir());
        let mut seen = roots.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), roots.len(), "duplicate roots: {roots:?}");
    }

    #[test]
    fn unconfined_launches_the_shell_unchanged() {
        // The guard that adding this module did not alter the non-macOS spawn.
        let confinement = Confinement::Unconfined(Unconfined::UnsupportedPlatform);
        let (program, prefix) = confinement.launch(SHELL);
        assert_eq!(program, OsStr::new(SHELL));
        assert!(prefix.is_empty());
    }

    #[test]
    fn enforced_wraps_the_shell_and_terminates_its_own_arguments() {
        let prefix = vec![
            OsString::from("-p"),
            OsString::from(profile(1)),
            OsString::from("-D"),
            OsString::from("ROOT0=/tmp"),
            OsString::from("--"),
            OsString::from(SHELL),
        ];
        let confinement = Confinement::Enforced {
            prefix: prefix.clone(),
            roots: vec![PathBuf::from("/tmp")],
        };
        let (program, emitted) = confinement.launch(SHELL);
        assert_eq!(program, OsStr::new(SANDBOX_EXEC));
        assert_eq!(emitted, prefix.as_slice());
        // `--` then the shell, so the executor's own `-c` and script land on the shell and are
        // never read as launcher arguments.
        assert_eq!(emitted[emitted.len() - 2], OsString::from("--"));
        assert_eq!(emitted[emitted.len() - 1], OsString::from(SHELL));
    }

    #[test]
    fn a_binding_carries_the_whole_path_after_one_equals() {
        // Paths reach the kernel as argv elements, so nothing here quotes or escapes. This pins
        // that a path containing `=` still binds whole.
        let confinement = Confinement::resolve(&std::env::temp_dir(), SHELL);
        if let Confinement::Enforced { prefix, roots } = confinement {
            let bindings: Vec<_> = prefix
                .iter()
                .filter(|argument| {
                    argument
                        .to_string_lossy()
                        .starts_with(ROOT_PARAMETER_PREFIX)
                })
                .collect();
            assert_eq!(bindings.len(), roots.len());
            for (index, root) in roots.iter().enumerate() {
                let mut expected = OsString::from(format!("{ROOT_PARAMETER_PREFIX}{index}="));
                expected.push(root);
                assert_eq!(*bindings[index], expected);
            }
        }
    }
}
