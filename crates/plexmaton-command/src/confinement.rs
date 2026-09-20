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

/// One cache a real build writes to outside the workspace: where it lives beneath the owner's
/// `HOME`, and the environment variables that move it elsewhere.
struct ToolchainCache {
    /// Where this cache lives when nothing relocates it.
    home_relative: &'static str,
    /// Variables that relocate it, each with the tail that reaches the cache inside the value.
    relocated_by: &'static [(&'static str, &'static str)],
}

/// The caches CMD-7 grants. A directory that does not exist is skipped rather than granted:
/// granting an absent path is the silent failure this module exists to avoid.
///
/// A set variable *replaces* its default rather than joining it. A relocated cache means the
/// default is not the one in use, so granting both would widen the zone past the caches this
/// names — a fence is only as narrow as the roots it hands the kernel.
///
/// `GOPATH` reaches its cache through `pkg/mod` on purpose. It names a workspace, holding `src`,
/// `bin` and `pkg`, so granting its root would make every Go project under it writable. That is
/// source, and source outside the admitted workspace is exactly what the fence exists to deny.
const TOOLCHAIN_CACHES: &[ToolchainCache] = &[
    ToolchainCache {
        home_relative: ".cargo",
        relocated_by: &[("CARGO_HOME", "")],
    },
    ToolchainCache {
        home_relative: ".rustup",
        relocated_by: &[("RUSTUP_HOME", "")],
    },
    ToolchainCache {
        home_relative: ".cache",
        relocated_by: &[("XDG_CACHE_HOME", "")],
    },
    ToolchainCache {
        home_relative: ".npm",
        relocated_by: &[("npm_config_cache", "")],
    },
    ToolchainCache {
        home_relative: "go/pkg/mod",
        relocated_by: &[("GOMODCACHE", ""), ("GOPATH", "pkg/mod")],
    },
    ToolchainCache {
        home_relative: ".local/share/uv",
        relocated_by: &[("UV_CACHE_DIR", "")],
    },
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
        if let Some(reason) = Self::unavailable() {
            return Self::Unconfined(reason);
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

    /// Why this host cannot fence a command at all, or `None` when it can.
    ///
    /// Independent of any workspace, so a composition root can settle it once. A policy that stops
    /// asking about commands is leaning on this answer: where it is `Some`, the question has
    /// nothing underneath it and must keep being asked.
    #[must_use]
    pub fn unavailable() -> Option<Unconfined> {
        if !cfg!(target_os = "macos") {
            return Some(Unconfined::UnsupportedPlatform);
        }
        if !Path::new(SANDBOX_EXEC).exists() {
            return Some(Unconfined::LauncherMissing);
        }
        (!launcher_applies()).then_some(Unconfined::ApplyRefused)
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

/// Character devices that carry no filesystem state and that ordinary programs open for
/// themselves. A global write deny covers `/dev` like anything else, and denying these does not
/// confine anything — it stops `git`, `python` and `curl` from starting, each reporting
/// `could not open '/dev/null'` rather than anything a reader would connect to a fence. Inherited
/// descriptors hide the problem: the executor opens `/dev/null` for stdin in the parent, so only a
/// command that opens one itself is affected, which is most of them.
///
/// Enumerated rather than granted as `/dev`, which holds raw disk devices.
const WRITABLE_DEVICES: &[&str] = &[
    "/dev/null",
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/tty",
    "/dev/dtracehelper",
    "/dev/autofs_nowait",
];

/// `(allow default)` then one global write deny, then the grants. Seatbelt takes the last matching
/// rule, so the grants must follow the deny.
fn profile(root_count: usize) -> String {
    let mut profile =
        String::from("(version 1)\n(allow default)\n(deny file-write* (subpath \"/\"))\n");
    profile.push_str("(allow file-write*");
    for device in WRITABLE_DEVICES {
        profile.push_str(&format!(" (literal \"{device}\")"));
    }
    // Process substitution and anything else addressing its own descriptors by path.
    profile.push_str(" (subpath \"/dev/fd\"))\n");
    for index in 0..root_count {
        profile.push_str(&format!(
            "(allow file-write* (subpath (param \"{ROOT_PARAMETER_PREFIX}{index}\")))\n"
        ));
    }
    profile
}

/// The workspace, both temporary directories, and the toolchain caches a build reaches — each
/// resolved, deduplicated, and dropped when it does not exist.
fn resolved_write_roots(workspace_root: &Path) -> Vec<PathBuf> {
    // macOS has two scratch directories and a command may write to either. `temp_dir` answers with
    // the per-user one `TMPDIR` names, under `/private/var/folders`; `/tmp` is the system one, and
    // a command that hardcodes it — which shell one-liners and build scripts routinely do — reaches
    // neither the other nor any granted root. Elsewhere the two are the same path and dedup drops
    // one.
    let mut candidates = vec![
        workspace_root.to_path_buf(),
        std::env::temp_dir(),
        PathBuf::from("/tmp"),
    ];
    let home = std::env::var_os("HOME").map(PathBuf::from);
    for cache in TOOLCHAIN_CACHES {
        let mut relocated = false;
        for (variable, tail) in cache.relocated_by {
            let Some(value) = std::env::var_os(variable) else {
                continue;
            };
            let path = PathBuf::from(value);
            if path.is_absolute() {
                candidates.push(if tail.is_empty() {
                    path
                } else {
                    path.join(tail)
                });
                relocated = true;
            }
        }
        if let (false, Some(home)) = (relocated, home.as_ref()) {
            candidates.push(home.join(cache.home_relative));
        }
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
    fn profile_without_roots_still_grants_the_stateless_devices() {
        // CMD-7: with no roots the fence denies every write that confines anything, and still
        // admits the character devices — denying those stops programs from starting rather than
        // bounding what they reach.
        let text = profile(0);
        assert!(text.contains("(deny file-write* (subpath \"/\"))"));
        assert!(!text.contains("(param \"ROOT0\")"));
        assert!(text.contains("(literal \"/dev/null\")"));
    }

    #[test]
    fn stateless_devices_are_granted_by_path_and_never_as_a_tree() {
        // `/dev` holds raw disk devices, so the grant is enumerated. `/dev/fd` is the one subtree,
        // for process substitution.
        let text = profile(1);
        for device in WRITABLE_DEVICES {
            assert!(
                text.contains(&format!("(literal \"{device}\")")),
                "{device} must be granted"
            );
        }
        assert!(text.contains("(subpath \"/dev/fd\")"));
        assert!(!text.contains("(subpath \"/dev\")"));
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
        // Both arms assert. A bare `if let` is a green no-op on a host that cannot fence, and this
        // test is cited as CMD-7 evidence: silence there would read as a passing binding claim.
        let Confinement::Enforced { prefix, roots } = confinement else {
            let (program, _) = confinement.launch(SHELL);
            assert_eq!(
                program,
                OsStr::new(SHELL),
                "an unfenced launch binds nothing"
            );
            return;
        };
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

    #[test]
    fn a_host_told_to_fence_can() {
        // CMD-7's platform claim is only checkable where it is made. Every other fence test steps
        // aside when this host has none, which is right for a developer inside an outer sandbox and
        // wrong for the gate: there, stepping aside would let the whole confinement story go
        // unexercised behind a green run, and nobody could see which of the two reasons it passed
        // for. The macOS gate sets this variable; nothing else does, so nothing else is asserted.
        if std::env::var_os("PLEXMATON_FENCE_REQUIRED").is_none() {
            return;
        }
        assert_eq!(
            Confinement::unavailable(),
            None,
            "this host was told it fences and cannot"
        );
    }

    #[test]
    fn both_temporary_directories_are_granted() {
        // CMD-7: macOS answers `temp_dir` with the per-user directory under `/private/var/folders`,
        // so a command hardcoding `/tmp` reached no granted root and its write failed with a denial
        // nothing in the profile explained.
        let roots = resolved_write_roots(&std::env::temp_dir());
        for scratch in [std::env::temp_dir(), PathBuf::from("/tmp")] {
            let resolved = std::fs::canonicalize(&scratch).expect("a scratch directory resolves");
            assert!(
                roots.iter().any(|root| resolved.starts_with(root)),
                "{} reaches no granted root: {roots:?}",
                resolved.display()
            );
        }
    }

    #[test]
    fn a_relocated_cache_replaces_its_default_and_never_grants_a_workspace() {
        // CMD-7 names caches. GOPATH names a workspace holding `src`, so granting its root would
        // make every other Go project under it writable; its cache is the `pkg/mod` beneath it.
        // A relocated cache also replaces its default rather than joining it, or a set variable
        // would widen the zone instead of moving it.
        let gopath = TOOLCHAIN_CACHES
            .iter()
            .find(|cache| cache.home_relative == "go/pkg/mod")
            .expect("the Go module cache is granted");
        assert_eq!(
            gopath.relocated_by,
            &[("GOMODCACHE", ""), ("GOPATH", "pkg/mod")],
            "GOPATH must reach the cache, never bind as its own root"
        );
        for cache in TOOLCHAIN_CACHES {
            assert!(
                !cache.relocated_by.is_empty(),
                "{} has no relocation, so its default can never be replaced",
                cache.home_relative
            );
        }
    }
}
