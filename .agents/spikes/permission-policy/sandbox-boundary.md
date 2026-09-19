# A per-command fence for the shell

| Field | Value |
| --- | --- |
| Read when | Scoping command containment, or choosing its mechanism |
| Status | Mechanism chosen and measured on macOS: cost, cancellation and failure modes. Production containment unbuilt |
| Corpus | Local revisions pinned in [the spike](./README.md); OS documentation checked 2026-09-05; probes re-run 2026-09-19 |

[Next decisions](./next-decisions.md) owns the position this serves. In short: the shell is the one
place where a policy sentence is not also a capability, and a fence is what closes that — not to add
safety on top, but so the command inspection that can never be complete is never written.

## What is enforced today, and what is not

Native file tools open every parent beneath a pinned root with no-follow descriptor semantics, so
their authority is narrower than the path string a model writes (WFS-1, MUT-2). The shell has none
of that: `command-tool.md` states its root is a starting directory rather than a sandbox, that an
approved shell reaches absolute paths, `..`, symlinks, network and inherited host authority, and
that the implementation claims no containment. Both sentences are honest. The second is the one this
work changes.

A native read rule can name project and scratch roots, but a usable shell also needs its
interpreter, system libraries, SDKs and toolchain caches. Redirecting `HOME`/`TMPDIR` alone imposes
no OS restriction, and CMD-2 already rejects removing `HOME`: it breaks user toolchains without
creating confinement.

## The mechanism, and why the alternatives lost

**Chosen: `/usr/bin/sandbox-exec` with a parameterised profile.** Zero first-party `unsafe`, no FFI,
an ordinary process spawn — and, measured below, it adds no process to the tree.

| Candidate | Why not |
| --- | --- |
| `sandbox_init_with_parameters(3)`, the C API | FFI to a deprecated symbol plus `pre_exec`, which is `unsafe`. `standards/rust.md` forbids first-party `unsafe` by default, and `pre_exec` runs between `fork` and `exec` in a multi-threaded process where any allocation can deadlock the child |
| `nono` SDK | `Sandbox::apply` confines the **calling** process, irreversibly. A TUI that must keep writing its journal and session store cannot be that process, and per-command confinement through it returns to `fork` + `pre_exec` + first-party `unsafe`. grok pins `=0.53.0` because a bump "can silently change [rule-emission order] and re-open the `mv x y && cat y` bypass with `is_applied()` still true", ships `bwrap` and per-spawn seccomp alongside it, and hand-maintains a glob parity guard across platforms — 6,010 lines for a scope that includes deny-inside-allow and read denial, both of which this position declines |
| Linux Landlock | In the kernel since 5.13, unprivileged, inherited by children, and architecturally the same shape: confine, then exec. Untestable on this machine. Additive path grants only — it cannot subtract a subpath from an allowed tree — with per-ABI rights, `chmod`/`chown`/`utime` outside coverage, and varying network coverage. Not a full write fence on its own |
| Linux bubblewrap | Mount-based boundaries, private temp, optional PID/network namespaces; expresses what Landlock cannot. Needs namespace availability, launcher distribution and process ownership, and its per-command cost is unmeasured. Untestable here |

Apple marks `sandbox-exec` deprecated in this host's `/usr/share/man/man1/sandbox-exec.1`. The
owner weighed that and accepted it: it is macOS's only unprivileged, per-command, in-tree
containment primitive, and Chrome, Codex and Claude Code all ship on the same interface. Deprecated
and universal is a different risk from deprecated and abandoned, and the probes below are what turn
a future OS change into a loud failure instead of a silent one.

Linux runs **unconfined, by decision**, until there is a host to test on. An abstraction over an
untestable platform buys exactly what grok's parity guard exists to defend against.

## Cost

[Containment cost](./containment-cost.md): a constant ~6 ms per invocation — 2.6× on `true`, 1.8× on
a pipeline, under a percent for anything doing real work. Cost is not a reason to defer containment
on this platform. Build throughput under a wrapper is still unmeasured; per-invocation cost is not
per-build cost.

## Cancellation, which is what made the change small

`seatbelt-lifecycle.py`, 24/24 on 2026-09-19. Every arm runs twice, bare and wrapped, because a bare
failure means the probe is wrong rather than the wrapper — the first two runs failed exactly that
way, on `$$` inside a subshell and on a zombie answering `kill(pid, 0)`.

The finding: **`sandbox-exec` applies the profile to itself and execs the target.** The probe records
`launched == shell`. There is no extra process, so the process group, group signalling, the drains
and the reaping are structurally identical to an unwrapped spawn.

| Evidence | Observed, bare and wrapped alike |
| --- | --- |
| SIGTERM and SIGKILL to the group | Launched process, shell and descendant all die within the bound |
| Output drain | Reaches EOF immediately; no surviving pipe holder |
| A descendant that calls `setsid(2)` | Survives both. The wrapper neither worsens nor fixes the escape CMD-5 already admits |

Every cancellation invariant in `command-tool.md` therefore holds untouched, and
`crates/plexmaton-command/src/executor.rs` needs no change below its spawn site.

## How a fence fails, and which failure matters

| Failure | Behaviour |
| --- | --- |
| Malformed profile | Loud: exit 65, a located parse error on stderr, command body never runs |
| Well-formed profile, **unresolved** subpath | **Silent**: accepted as valid, exit 0, command runs, fence grants nothing |

The second is the whole hazard, and macOS hands it to you by default — the temp root arrives as
`/var/...` while the kernel matches `/private/var/...`. A write fence that looks applied and denies
the roots it was told to allow is worse than no fence, because it reads as working.

The defense is cheap and testable: canonicalize every path before it enters a profile, and reject at
build time any path not equal to its own resolved form.

## Host probe

```sh
python3 .agents/spikes/permission-policy/seatbelt-probe.py      # 13 correctness checks
python3 .agents/spikes/permission-policy/seatbelt-lifecycle.py  # 24 lifecycle checks
```

Both pass on macOS 26.6.2, 2026-09-05 and again 2026-09-19. Run them outside any outer sandbox: a
nested `sandbox_apply` is refused, and every wrapped arm then fails for a reason unrelated to the
question. No production process, user configuration, reference suite or model endpoint is used.

| Evidence | Observed |
| --- | --- |
| Startup and positive controls | Profile accepted; unsandboxed fixture child can write outside and to `.git` fixtures and reach the owned loopback listener |
| Writable roots | Project and private scratch writes succeed |
| Restricted writes | Direct outside write, `.git` write, symlink escape and nested-shell outside write all fail without creating the target |
| Network sample | Child TCP connection to the same loopback listener fails |
| Deliberate limitation | Reading the synthetic outside file succeeds: this is a **write**-boundary experiment |

Reads stay open by design. Read confinement is what breaks builds — every compiler wants SDKs,
sysroots and headers, discovered one failure at a time — and it buys little once the model API call
is the largest egress channel any file the agent reads already travels through.

The profile starts with `allow default` and adds restrictions. It is not a general security
boundary: IPC, process access, inherited descriptors, hard links, path-replacement races, restricted
reads and comprehensive networking remain unproven, and the spike claims none of them. The real
regression these probes detect is a missing write fence or a launch that bypasses the profile. An
in-memory mutation removing `deny file-write*` fails the outside-write assertion as intended.

## What remains

1. **The write-root list.** Workspace root, `TMPDIR`, and the toolchain caches a real build reaches
   under `HOME`. Derive it by running this repository's own `cargo test` under the fence and adding
   what is denied, rather than guessing a directory layout. Its failure mode is a clean denied
   write, which is what makes iterating safe.
2. **Launch failure stays separate from command exit.** Generic `Permission denied` on stderr cannot
   identify a sandbox violation; a fence that fails to apply on macOS must fail the command with its
   own typed cause rather than running unconfined.
3. **Confinement as a recorded fact.** macOS fenced and Linux not must be visible per command, or
   work that succeeds on both succeeds for different reasons and nobody can see which.
