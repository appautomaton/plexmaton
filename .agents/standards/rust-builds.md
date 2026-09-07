# Standard — Rust builds

| Field | Value |
| --- | --- |
| Trigger | Building in a task worktree; changing Cargo profiles, artifact paths, caches or concurrency |
| Owns | Local build profiles, artifact isolation, cache scope and parallel build budgets |

## Artifact ownership

Every task worktree uses its own default `target/`. Keep both Cargo's final target directory and
intermediate build directory isolated. Do not share `CARGO_TARGET_DIR` or `build.build-dir`, copy a whole warm target, or hardlink mutable
artifacts. The [dependency seeder](#dependency-seeding) is the supported selective-clone boundary.
Two checkouts can produce the same member fingerprint and reuse the wrong code; see
[cargo#12516](https://github.com/rust-lang/cargo/issues/12516). A compiler cache and a shared Cargo
target are different mechanisms.

Use the existing Cargo home for downloaded registry/Git sources; a fresh worktree does not need
its own Cargo home. Keep toolchains pinned under [Rust](./rust.md#toolchain). Do not change global
rustup or Cargo configuration to tune this project.

`git worktree remove` retires the task's `target/` under
[Git workflow](./git-workflow.md#merge-and-retire). Build output should not outlive its owning task.
Preserve durable evidence in tracked files and private runtime state outside that disposable
directory. For a paused task with costly artifacts, the owner may run `cargo clean` in its idle
worktree; preserve the source and branch. Do not sweep another task's artifacts by age or size.
Any explicitly created external experiment target needs the same owner and cleanup.

The PTY smokes expect `<worktree>/target/debug/plexmaton` and write under `target/smoke`; keep that
default for executable validation. After moving a worktree, rebuild packages whose fixtures embed
old absolute paths (`cargo clean -p <package>`). Never treat a relocated binary as current evidence.

## Profiles

[Cargo.toml](../../Cargo.toml) sets dev debug information to `line-tables-only`; test inherits dev.
Filename/line backtraces remain available, while debugger variables and parameters are omitted.
Local incremental compilation stays enabled for fast edits. Optimizations, assertions, overflow
checks, panic behavior and release settings retain their Cargo defaults.

For an investigation requiring full debugger data, opt in for the affected build:

```console
CARGO_PROFILE_DEV_DEBUG=2 CARGO_PROFILE_TEST_DEBUG=2 cargo test -p <crate> --locked
```

For an explicitly one-shot local build, `CARGO_INCREMENTAL=0` avoids retaining incremental state.
Do not make that the interactive default: the [measured small edit](../spikes/rust-worktree-builds/README.md)
rebuilt substantially slower without it. CI's pinned toolchain action already sets
`CARGO_INCREMENTAL=0`; no duplicate override or extra CI profile is needed.
Profile changes can leave older artifacts behind, so keep experiments in owned, disposable targets.

[Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) define inheritance and
debug/incremental semantics. The spike records the workload, result and limits; local timings do
not establish GitHub runner speedups.

## Parallel work

Parallel agents can edit and review independently; simultaneous compiler processes still share
one machine. Before launching concurrent local builds, divide a CPU budget across the Cargo
invocations and pass `--jobs N`. Do not give every worktree all logical CPUs. For example, on an
18-CPU host, three builds with `--jobs 6` share an 18-job budget; use fewer jobs if memory or other
active work requires it. Do not commit that machine-specific value as a project-wide default.

`--jobs` limits compilation. When executing several test binaries concurrently, also divide test
threads with `-- --test-threads N`; a compile limit does not constrain the test runner. Coordinate
the invocations the task owns and account for other active builds; a worktree count alone is not
a measure of CPU demand. See [Cargo build jobs](https://doc.rust-lang.org/cargo/commands/cargo-build.html)
and [test execution](https://doc.rust-lang.org/cargo/commands/cargo-test.html).

Choose the smallest meaningful package/target under [quality gates](./quality-gates.md#the-lanes).
Do not prebuild every fresh worktree or rerun the full workspace locally before sending it to CI.
One Cargo command can already compile in parallel; simultaneous commands in the same target
directory add lock contention. GitHub runners keep Cargo's runner-sized default job count.

## Build reuse

Cargo reuses unchanged crate artifacts within the task's target directory. With incremental
compilation enabled, rustc also reuses unaffected work inside a changed crate. An empty target
builds its required artifacts independently unless explicitly prewarmed below; sharing downloaded
sources through Cargo home does not share compiled output.

Use ordinary Cargo commands without an external compiler-cache wrapper. Retain the task's target
through its edit loop, then retire it with the worktree. Focused builds and the dev profile reduce
its footprint; task retirement bounds its lifetime. Do not periodically clean active targets
merely to shrink a directory: rebuilding them can increase writes.

Rejected: adding compiler-cache infrastructure for fresh worktrees, because the measured startup
savings did not justify additional configuration, lifecycle management and debug-info failure
modes; sharing mutable Cargo targets, because another checkout's code can be reused; and disabling
incremental compilation for every local edit, because the measured rebuild penalty outweighs
its additional saving.

## Dependency seeding

[`seed-rust-deps.py`](../../scripts/seed-rust-deps.py) can prewarm an absent task target on macOS
with Cargo 1.98 and a clone-capable filesystem. It clones eligible external Rust libraries and
their fingerprint state into independent files. Local/path packages, build-script/native-link/
proc-macro packages and their resolved dependents are excluded; Cargo rebuilds those normally.
This conservative command has a smaller reuse set than the
[initial prototype](../spikes/rust-dependency-seeding/README.md).

Save a trusted ordinary-Cargo build's JSON stdout outside its target.
Keep the owned source checkout and target available until seeding completes. For example,
from that owned worktree (choose jobs from the shared CPU budget):

```console
cargo test -p plexmaton-tui --lib --no-run --offline --locked --jobs 6 --message-format=json > /private/tmp/owned-build.jsonl
python3 scripts/seed-rust-deps.py --source "$PWD" --build-log /private/tmp/owned-build.jsonl --destination /absolute/path/to/new-worktree
```

Both workspaces must have equal lockfiles and external package identities/source locations.
External sources must live outside both checkouts. Use local default target/build directories
and installed Cargo 1.98. The destination target must be absent.
Only host-debug build/test library logs are supported. Cargo remains responsible for unit freshness, including profile/feature changes;
a seed is not proof that every cloned artifact will be reused. The tool does not attest the
producer log's provenance or make arbitrary build inputs hermetic.

The command takes existing Cargo source locks without waiting, refuses symlinked logs/artifacts and unknown
output layouts, and publishes a private staging directory with no-replace atomic rename.
A busy producer, unsupported input, clone failure or racing destination leaves existing output
intact. Cancellation removes only the owned unpublished stage; committed targets remain complete. An uncatchable termination
may leave ignored `.target-seed-*` output for its task owner to retire. No lock file, incremental
state, executable or build-script output is seeded; there is no copy/hardlink fallback.

Run the normal Cargo command in the new worktree after seeding. On refusal, use ordinary Cargo
with its private target; do not delete an existing target just to make the prewarm command succeed.
Worktree creation does not invoke this helper automatically, and it writes no global configuration or installs toolchains.
