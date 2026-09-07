# Selective dependency seeding

| Field | Value |
| --- | --- |
| Read when | Implementing or evaluating dependency prewarming for a new worktree |
| Question | Can external compiled dependencies be reused without sharing mutable target state? |
| Status | Conservative project command implemented and locally qualified |

## Initial signal

On 2026-09-07, Plexmaton `daee70c`, Rust/Cargo 1.98.0, macOS/APFS and six compiler jobs,
`cargo test -p plexmaton-tui --lib --no-run --offline --locked` accepted a selective APFS seed.
The prototype joined Cargo artifact events to full metadata package IDs, excluded local/path
packages, and cloned exact dependency, fingerprint and build-script files into private targets.
Clone operations required distinct inodes and preserved mtimes. There were no compiler wrappers.

| Condition | Instrumented elapsed | Observed process writes |
| --- | ---: | ---: |
| Empty target after host-cache warmup | 16.320 s | 618.57 MiB |
| Dependency seeding | 0.342 s | 0.14 MiB |
| Seeded target after donor removal | 7.823 s | 178.55 MiB |

The seed contained 970 files and 285.70 MiB of logical data from 105 external packages.
Cargo reported 124 external compiler artifacts fresh and rebuilt four local units. The donor
checkout and target were removed before the second consumer build; all 402 TUI tests passed.
Two local source edits returned distinct expected markers, rebuilt one local unit each and
retained external reuse. Source-line backtraces passed; the debug map had zero warnings.
[Initial results](./initial-results.json) retain exact counters and scope.

## Measurement limits

These are ordered individual observations, with an empty-target control after system-cache
warmup; host activity was not controlled. A scoped observation library recorded process-entry/
exit `proc_pid_rusage` write counters. Calibration recorded 32 MiB for a synced 32 MiB write,
and zero process-accounted data I/O for its APFS clone. Collection is not proven complete for
all child images. The counts do not represent unique APFS extents, device traffic or NAND writes.
Do not sum `du` values to claim unique disk usage of cloned files.

## Admission limit

This workload's build-script events contained no linked paths or exported environment entries.
The result does not establish generic build-script, native dependency, proc-macro, feature,
profile, toolchain or concurrent-publication compatibility. The project command must establish
its supported boundary independently; the broad prototype's savings do not transfer automatically
to a more conservative implementation. No daily build setting has been changed.

## Project command qualification

The [project seeder](../../../scripts/seed-rust-deps.py) excludes build-script/native-link and
proc-macro packages plus their resolved dependents. This shrinks the eligible set to 47 external
packages and 48 library units; 336 files / 78.69 MiB were cloned. No script execution state,
executables or local compilation state enters the seed.

On source `1393bcb` with the same TUI command and profiles:

| Current command condition | Instrumented elapsed | Observed process writes |
| --- | ---: | ---: |
| Final empty-target control after host-cache warmup | 10.594 s | 622.48 MiB |
| Seeding | 0.403 s | 0.51 MiB |
| Seeded consumer after donor removal | 9.892 s | 493.12 MiB |

Including seeding, the final consumer took 10.295 s: no material wall-time benefit is established.
Observed process writes were about 21% lower. APFS shared the 78.69 MiB of selected dependency
data, but unique physical retained size was not measured. This command is a manual write/space
optimization; the broad prototype's 50%/71% result is not its adoption claim.

The initial empty target took 16.500 s, and earlier seeded samples took 13.515 s and 12.614 s
before seeding overhead. That variation is why a later warm-host empty-target control was added.
[Command results](./command-results.json) retain all observations and exact unit counts.
These remain ordered single samples with uncontrolled host activity, not a causal or statistical
speedup guarantee. The first seeding byte sample was rejected because rustup exec transitions
produced incomplete image accounting; only direct-Cargo samples are used. Counter limits above apply.

All 409 current TUI tests passed after the source checkout and target were removed. The
[real Git/Cargo fixture](../../../scripts/tests/test_seed_rust_deps_cargo.py) also verifies reuse
of a pinned external Git dependency, distinct consumer source identity, full-debug external
source frames after donor removal, changed dependency features and subsequent local edits.
[Filesystem/policy tests](../../../scripts/tests/test_seed_rust_deps.py) cover exclusion closure,
exact unit ownership, bounded regular-file logs, source locks, clone independence, races, failure,
real SIGTERM cancellation and the committed publication boundary. The real Cargo fixture also
holds a build-script barrier while the command proves it refuses Cargo's active target lock.

These checks support the explicit host-debug command boundary. Native/build-script/macros and
execution-dependent consumers are rebuilt normally. There is no automatic activation, source
binary rewriting, global setup or reuse guarantee for arbitrary Cargo commands and layouts.

Shared external source locations must match the producer log and both metadata graphs, and live
outside both workspaces. Worktree-local vendoring and relocated source stores are refused.
