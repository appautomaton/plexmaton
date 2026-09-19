# Spec — Foreground command tool

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Strict command admission, foreground Unix process-group ownership, per-command write confinement, bounded output capture and typed completion |
| Depends on | [tool-admission](./tool-admission.md) APV-1 through APV-3; [agent-loop](./agent-loop.md) LOOP-2 |
| Proven by | `plexmaton-command::{admission,capture,confinement,executor}`, `plexmaton-runtime::runtime::tests::tools`, and `plexmaton-tui::frames` tests |

## Invariants

**CMD-1 — Admission fixes the operation.** The provider-neutral strict `exec_command` schema
requires `cmd` and nullable `timeout_ms`, and refuses additional properties. Admission accepts a
required non-empty `cmd` string bounded to 6,144 Unicode scalars and 24 KiB of UTF-8, plus a
missing, null or non-zero `timeout_ms` no greater than the hard ceiling. Those limits leave room
for worst-case JSON escaping and the bounded root inside the 64 KiB raw-call and 65 KiB canonical
ceilings. Admission pins the definition revision, root, timeout and
`[FileRead, FileWrite, ProcessSpawn]`; the executor revalidates them and the root's file identity
(APV-1 through APV-3). Approval detail bounds each root and command with head, tail and exact
omitted-byte count; the command leads so execution is visible before context wraps. Transcript
invocation retains the original canonical command, root and timeout as `ToolDetail::Command` within
admitted-state bounds. Display summaries do not become the source for inspection or copying (APD-1).

**CMD-2 — The process context is explicit and noninteractive.** One `/bin/sh -c` root starts in its
own process group, with null stdin and the admitted root as `cwd`. The tool snapshots the owner's
OS-string environment once, removes every configured provider's exact API-key variable (MDL-3) plus
Plexmaton-private and credential-shaped variables, then uses a cleared child environment to install
that snapshot plus canonical `PWD` and fixed no-colour, noninteractive pager and prompt controls.
Real `PATH`, `HOME`, locale and toolchain configuration survive; `OLDPWD` does not. There is no PTY,
background API or `write_stdin` path.

Rejected: removing `HOME` while approved commands retain filesystem authority; it breaks user
toolchains without creating confinement.

**CMD-3 — Retention cannot stop drainage.** Stdout and stderr have independent owned drains which
keep reading after retention fills. Each retains at most 64 KiB of raw-byte head and tail with the
exact bytes read and omitted-byte count. After root and owned-group completion, EOF gets a bounded
grace; expiry seals both drains cooperatively and returns the partial captures instead of waiting
forever on an escaped pipe holder. A second aggregate byte bound applies after lossy UTF-8
conversion, so invalid input cannot expand the model result past its limit. Only the final typed
result exposes output; transcript detail is the exact model text plus omission metadata.

**CMD-4 — Completion is typed.** A result distinguishes an exit code, a Unix signal, timeout and
cancellation. Text and numeric conventions are presentation, never lifecycle control flow.

**CMD-5 — Stop settles owned-group authority before returning.** Cancellation wins a simultaneously
observable timeout; the hard deadline wins over a simultaneously ready completion. Either stop
sends `SIGTERM` to the process group and returns early if it becomes quiescent; at grace expiry any
remaining group or unreaped root receives `SIGKILL`, then the root is reaped, group disappearance is
bounded, and both drains join. A naturally exited root follows the same sequence for descendants.

**CMD-6 — Cleanup is an explicit awaited transition.** The caller drives execution to completion
and cancels through its token; there is no fire-and-forget task and no detached `Drop` reaper.
The live runtime retains the execution worker across cancelled event polls; interrupt and shutdown
cancel and join it before returning, while direct runtime `Drop` does the same as a resource-safety
backstop. Partial output streaming remains outside this boundary.

Rejected: detached Tokio tasks in place of joined workers, which would make direct `Drop` abandon
cleanup. A later bounded supervisor may pool workers without changing this ownership rule.

**CMD-7 — A launch is confined, or the result says it is not.** On macOS the shell is launched
through a Seatbelt profile that denies writes outside a resolved root set — the admitted workspace
root, the temporary directory, and the toolchain caches the owner's environment names — and leaves
reads, network and process operations untouched. Stateless character devices are granted by name,
because denying them confines nothing and instead stops ordinary programs from starting: `git`,
`python` and `curl` all open `/dev/null` for themselves, and the inherited stdin CMD-2 supplies
hides that from any test that only redirects into a file. The launcher applies the profile to itself and
`exec`s the shell, so the spawned process is the shell and CMD-2's process group, CMD-5's signalling
and CMD-3's drains are unaffected. Every root enters the profile as its own resolved form or does
not enter it: an unresolved subpath compiles to a valid profile, exits zero and grants nothing.
Where no profile can be applied — off macOS, without the launcher, or inside an outer sandbox that
refuses nesting, which is probed once per process rather than per command — the spawn is byte
-identical to an unconfined one and the typed result carries which case applied. The command itself
is never inspected.

Rejected: matching commands for dangerous shapes, which any interpreter defeats under a different
spelling and which cannot establish a command's effects in any case; and confining reads, which
breaks toolchains one missing sysroot at a time while the model request is already the widest path
off the machine.

## Workspace-root limit

The canonical root is a starting directory, not the fence. CMD-7 bounds writes where it applies;
everything else an approved shell reaches is unchanged — absolute paths, `..`, symlinks, network and
inherited host authority. Environment scrubbing is secret hygiene; files and sockets remain. A
descendant may escape group signalling with a new session; bounded drainage prevents retention but
does not terminate it, and the fence neither worsens nor repairs that escape. Off macOS there is no
fence, by decision: Landlock is in the kernel and bubblewrap sits beside it, but neither can be
exercised on the host this was built on, and a fence that is never run reports confinement it may
not deliver.

The root's device and inode are rechecked immediately before spawn. POSIX offers no safe,
first-party-`unsafe`-free way to make the final check and `current_dir(path)` one atomic operation,
so a same-user path replacement in that final interval is outside the guarantee. This is a
path-rooted execution boundary with stale-root detection, not descriptor-rooted confinement.

The implementation currently supports Unix only. The fixed shell path and signal numbers are
covered on the host gate; no Windows job-object adapter exists.

## Evidence

[Named proofs](../evidence/command-tool.md), one row an invariant.
