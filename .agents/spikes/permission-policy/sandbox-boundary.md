# Rule-based approval and a small command sandbox

| Field | Value |
| --- | --- |
| Read when | Scoping command containment, automatic approval or shell permission matching |
| Status | Rule-based priority agreed; macOS fixture probe passed; production containment unproven |
| Corpus | Local revisions pinned in [the spike](./README.md); OS documentation checked 2026-09-05 |

## First implementation boundary

The user prioritizes deterministic action determination and approval. Implement rules and scoped
grants first; a future `auto` reviewer can consume unresolved decisions at the existing approval
boundary. No LLM classifier, unused mode setting or speculative reviewer framework is needed now.
Explicit deny/ask precedence remains P3. If command containment is later added, the executor applies
its separately resolved policy and grants must bind that identity alongside P4's operation and
environment identities. Approval under containment cannot authorize an unconfined retry.

Native file tools already have WFS-1 confinement. The shell does not (CMD-2). A native read rule
can name project and scratch roots, but a usable shell also needs its interpreter, system libraries,
SDKs and selected toolchain/cache paths. Redirecting `HOME`/`TMPDIR` alone imposes no OS restriction.

Command sandboxing and its shared interface remain optional. Keep policy and execution separate;
choose interfaces and backends only if needed. Rejected: making future integration room a required
abstraction in the first rule-based delivery. No placeholder trait, mode or dependency is needed.

If containment is pursued, `crates/plexmaton-command/src/executor.rs:58` is a candidate insertion
point before shell code starts. Choose the interface from the actual backend requirements then;
keep the provider, TUI and policy owner outside the child, and prove cancellation and reaping.

## What the source comparison establishes

| Candidate | Small useful scope | Work that the wrapper does not remove |
| --- | --- | --- |
| macOS Seatbelt via `sandbox-exec` | Per-command write roots, protected subpaths and network rules without a daemon | Apple marks the executable deprecated in this host's `/usr/share/man/man1/sandbox-exec.1`; profile compatibility and least-privilege reads need evidence |
| Linux Landlock helper | Unprivileged restrictions on handled file operations, inherited by child execution | Probe ABI/features; additive path grants cannot directly subtract `.git` from an allowed workspace tree; metadata operations remain outside its coverage |
| Linux bubblewrap | Mount-based read/write boundaries, private temp, optional PID/network namespaces | Namespace availability, launcher distribution, profile correctness and process ownership |
| `nono` SDK | Common Landlock/Seatbelt vocabulary | Still needs a correctly placed launcher, dependency audit and explicit handling of incomplete enforcement; an SDK does not establish equal platform guarantees |

The [Linux kernel documentation](https://www.kernel.org/doc/html/latest/userspace-api/landlock.html)
describes per-ABI rights, irreversible inheritance and current gaps including `chmod`, `chown` and
`utime`. Network coverage also varies by ABI. An allow-list model is not a promise to block every
filesystem effect or all communication. Do not claim a full write fence from Landlock alone.

[Bubblewrap](https://github.com/containers/bubblewrap#sandbox-security) leaves policy to its caller;
the launcher needs no Docker daemon or VM. Package metadata checked 2026-09-05 reports:
[Debian amd64 0.12.0-1~deb13u1](https://packages.debian.org/trixie/bubblewrap), 55.5 kB download /
142.0 kB installed; [Arch x86_64 0.12.0-1](https://archlinux.org/packages/extra/x86_64/bubblewrap/),
41.7 KB / 97.4 KB. These are package sizes including support files, excluding shared dependencies;
they are neither standalone binary sizes nor process memory measurements. No startup or build
overhead was measured. Versioned dependency/runtime audits remain necessary before adoption.

DSH's `packages/sandbox/sandbox-local/src/profiles.ts:16,30,51` shows a small per-command wrapper:
bubblewrap mounts, Landlock grants, or Seatbelt rules. Its Linux Landlock profile reads `/` and its
shell package explicitly limits the guarantee to file effects. The smaller profile does not prove
private reads, network isolation or protected control paths. `sandbox-local/src/index.ts:490`
selects a runner with explicit unavailable handling. This boundary fits Plexmaton better than
copying an entire agent's startup sandbox.

Grok pins `nono = 0.53.0`; `xai-grok-sandbox/src/lib.rs:195` logs and continues when the OS sandbox
is unavailable or application fails. Its `profiles.rs:330` uses Seatbelt for write-deny exceptions
and bubblewrap on Linux. Those choices are not evidence that plain Landlock supplies equivalent
exceptions. Plexmaton's proposed confined launch must fail before the command starts when required
enforcement is unavailable; unconfined execution requires a separately authorized operation.

## Host probe

Run from this worktree:

```sh
python3 .agents/spikes/permission-policy/seatbelt-probe.py
```

On macOS 26.6.2, all 13 checks passed on 2026-09-05. The outer agent sandbox refused nested
`sandbox_apply`; the same disposable probe passed with execution outside that outer sandbox.
No production process, user configuration, reference suite or real model endpoint was used.

| Evidence | Observed |
| --- | --- |
| Startup and positive controls | Profile accepted; unsandboxed fixture child can write both outside and `.git` fixtures and connect to the owned loopback listener |
| Writable roots | Project and private scratch writes succeed |
| Restricted writes | Direct outside write, `.git` write, symlink escape and nested-shell outside write fail without creating the target |
| Network sample | Child TCP connection to the same loopback listener fails |
| Invalid profile | Command never creates its marker |
| Deliberate limitation | Reading the synthetic outside file succeeds: this is a write-boundary experiment |

The profile starts with `allow default` and adds restrictions. It is not suitable as a general
security boundary: IPC, process access, inherited descriptors, hard links, path replacement races,
restricted reads and comprehensive networking remain unproven. No production cancellation test
or performance measurement was run. The real regression this probe detects is a missing write
fence or a launch that bypasses the profile; parent controls distinguish denial from fixture failure.
An in-memory mutation removing `deny file-write*` failed the outside-write assertion as intended.

## Remaining bounded work

Broad comparison is complete enough to start rule-based permission work. Command matching needs
integration evidence; containment questions apply only if that work is pursued:

1. **Command matching.** Start with exact scripts and explicit roots/environment. If prefix grants
   become necessary, exercise a bounded parser corpus covering wrappers, chaining, redirection,
   substitution and unsupported syntax returning Ask. `cargo test` can run repository code even
   when its visible command is unchanged; a rule cannot infer that code's full effects.
2. **Usable confinement.** Prove one chosen macOS profile with a tiny offline Rust build, explicit
   toolchain read paths, owned cache/scratch writes, protected control paths and blocked outside
   reads. Measure startup and representative build overhead separately. Linux needs its own host
   matrix and may reasonably use bubblewrap when Landlock cannot express the promised boundary.
3. **Lifecycle and explanation.** Pin policy before spawn; reject stale grants and unsupported
   containment; preserve cancellation and cleanup. Keep launcher startup failure separate from
   command exit. Generic `Permission denied` stderr cannot reliably identify a sandbox violation.

The first delivery implements rules and grants. It neither requires nor commits to a sandbox
interface or backend. If containment is later chosen, read compatibility, permissions and lifecycle
must compose; package size alone does not determine that work.
