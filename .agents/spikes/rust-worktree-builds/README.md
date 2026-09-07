# Rust worktree builds

| Field | Value |
| --- | --- |
| Read when | Reconsidering local Cargo profiles, artifact retention or compiler caching |
| Question | Which small build changes reduce each task's disk footprint while preserving fast edits? |
| Status | Measured; dev profile selected; the PR owns broad GitHub validation |

## Evidence

On 2026-09-07 at base `9431d23`, the primary `target/` occupied about 40 GiB and an active task
target about 8.6 GiB. The primary included accumulated debug artifacts, incremental state and a
historical `target/verify-d09f094`. Those directories were inspected, not cleaned or reused for
this experiment. Their accumulated sizes are not a fresh-build baseline.

The workspace had no custom profiles, wrapper or Cargo build configuration. No compiler cache
was installed, and relevant environment overrides were absent. The existing
[CI run](https://github.com/appautomaton/plexmaton/actions/runs/34085005068) showed that the pinned
toolchain action already sets `CARGO_INCREMENTAL=0` and `RUSTFLAGS=-D warnings` on GitHub.

Three fresh, separate targets used the same TUI library-test compilation workload on macOS
Apple Silicon, 18 logical CPUs, Rust/Cargo 1.98.0, with six compiler jobs. Registry sources were
already downloaded; compiled artifacts were cold. Cases ran sequentially, one sample each;
unrelated host activity was not controlled.

```console
cargo test -p plexmaton-tui --lib --no-run --offline --locked --jobs 6 --target-dir <fresh-target>
```

The edit build temporarily added `#[inline(never)]` to `Palette::style`, changing codegen without
changing the method's result. The original file was restored byte-for-byte. Sizes are allocated
bytes from `du -sk` over each target, including incremental state, converted to MiB below.

| Profile | Fresh build | Fresh target | Small edit build | Target after edit |
| --- | ---: | ---: | ---: | ---: |
| Full debug, incremental | 11.519 s | 617.8 MiB | 1.138 s | 706.0 MiB |
| Line tables, incremental | 11.610 s | 520.7 MiB | 1.007 s | 605.0 MiB |
| Line tables, no incremental | 10.782 s | 407.4 MiB | 4.682 s | 423.3 MiB |

[Raw results](./results.json) retain byte counts and elapsed seconds. Line tables reduced the
fresh target by 15.7% and the post-edit target by 14.3% compared with full debug information.
Disabling incremental compilation reduced storage further, but this edit took about 4.6 times
as long as the line-table incremental case. Cold-build timings are too close to claim a speedup.

## Decision and limits

[Rust builds](../../standards/rust-builds.md) owns the adopted profile, concurrency and retention
rules. The selected dev profile keeps incremental compilation and uses line-table debug info;
full debugger variable data remains available through a per-command override. Task retirement
removes its target; the profile change does not reclaim old targets in other checkouts.

This is one local compilation workload, not the whole workspace, a benchmark of sustained parallel
builds, or a GitHub speed measurement. The measurement command did not execute tests. No changes
were made to dependency versions, compiler optimization, debug assertions, release settings,
global configuration or the other worktree. A shared compiler cache was not measured or installed.
Its admission requires demonstrated cache hits and savings without losing checkout-specific paths.

With the selected profile in Cargo.toml and the original source restored, all 389 TUI library
tests passed locally using six compiler jobs and six test threads. Citation, document-budget,
typo and diff checks passed. The PR supplies whole-workspace and terminal validation.

## Reproduce

In a fresh task worktree with downloaded dependencies and no edits to `theme.rs`:

```console
python3 .agents/spikes/rust-worktree-builds/measure.py --jobs 6
```

The [measurement script](./measure.py) sets debug/incremental options explicitly for each case,
refuses an existing `target/profile-measure`, and restores the temporary source edit on exit.
Build logs and results go under that directory; retain useful results before retiring the worktree.
The job count is an explicit workload parameter, not a project-wide default.
