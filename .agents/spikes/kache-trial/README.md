# Kache worktree trial

| Field | Value |
| --- | --- |
| Read when | Enabling or revisiting compiler caching, or interpreting Rust build write measurements |
| Question | Can fresh worktrees reuse compiled dependencies with fewer writes and correct paths/backtraces? |
| Status | Guarded local opt-in supported; ordinary Cargo remains wrapper-free |

## Result

On 2026-09-07, base `2661aef`, macOS 26.6.2/APFS, 18 logical CPUs, Rust/Cargo 1.98.0 and six
compiler jobs, the guarded cache mode reused 106 dependency compilations in a fresh consumer
worktree. Kache reported 277,470,781 reflink-restored bytes and zero copied bytes.
First-party incremental compilation remained local.

| TUI library-test compilation | Official binary wall time | Metered process writes |
| --- | ---: | ---: |
| Plain Cargo, fresh target | 9.126 s | 618.5 MiB |
| Kache, populate cache | 12.884 s | 630.0 MiB |
| Kache, reuse in another worktree | 6.897 s | 201.9 MiB |
| Plain Cargo, median of three small edits | 0.939 s | 112.5 MiB |
| Kache, median of the same edits | 0.918 s | 112.6 MiB |

[Results](./results.json) retain exact counts, per-executable aggregates and calibration values.
The fresh consumer wrote 67.4% less instrumented process I/O than the plain fresh build; its
unmodified-binary command took 24.4% less time. Cache population cost more, and the edit loop
showed no material saving. These are one sample per fresh condition and three distinct edits,
not confidence intervals or a claim about the whole workspace, GitHub CI or SSD endurance.

Workload: `cargo test -p plexmaton-tui --lib --no-run --offline --locked --jobs 6`, separate targets,
registry sources available. Edits changed one `Palette::style` codegen attribute, then restored the
source. Runs were sequential; host activity was uncontrolled. Times exclude daemon start/stop;
I/O includes observed daemon activity.

## Correctness and decision

| Boundary | Observed result |
| --- | --- |
| Checkout-specific `CARGO_MANIFEST_DIR` and runtime file | Each fixture used its own root, including after the producer root was removed |
| Changed source, `include_str!` and `env!` inputs | The rebuilt consumer returned the new values |
| Failed compilation, then valid source | Failure was not replayed as success; recovery returned correct output |
| Concurrent same-name/version builds with different source | Both returned their own values and the shared dependency's correct value |
| Cancellation while a cache miss awaited its compiler | An owned FIFO barrier made the interruption deterministic; the next build completed correctly |
| Real consumer after deleting producer checkout | All 389 TUI tests passed with the official binary's guarded outputs |
| Default Kache executable path | Three application source-line frames became zero on both a fresh compile and a cache hit |
| Guarded mode with ordinary linker discovery | All three application source-line frames remained |

Application backtrace frames:
[plain](./bt-plain-backtrace.txt), [default miss](./bt-cache-a-backtrace.txt),
[default hit](./bt-cache-b-backtrace.txt), [guarded](./stock-linker-backtrace.txt).

Kache's default miss path strips Cargo's incremental argument. On this macOS profile its cached
executable path also injects `-Wl,-oso_prefix,...`; the resulting debug-map warnings corresponded
to the missing frames. `KACHE_PRESERVE_INCREMENTAL=1` bypasses artifact caching for incremental
units and uses `incremental.kache-preserved` beside the original directory. The guarded mode also
sets `KACHE_CACHE_EXECUTABLES=0`. Disabling executable caching alone relies on adaptive behavior;
it is not the tested two-setting recipe. A packed-debug-info probe did not recover the frames.

[Rust builds](../../standards/rust-builds.md#compiler-cache-boundary) owns the adopted policy.
Release, non-incremental first-party code, other platforms, IDE integration and sustained saturated
builds remain unverified. No global settings, login service or project dependency was added.

## Measurement boundary

`time -l` reported zero block-output operations for a confirmed 64 MiB write, so it was rejected
as a byte counter. A separate live/zombie probe showed that `proc_pid_rusage` does not roll a
child's disk-byte count into its parent. The trial therefore used [a scoped library](./io-account.c)
to record each instrumented image's entry/exit `ri_diskio_byteswritten`, keyed by PID/start time,
and summed those deltas. Unix datagrams keep the measurement log writes outside those processes.

[Calibration](./io-calibration.c) reported exactly 32 MiB for both fsynced and ordinary 32 MiB
writes, and zero file-data I/O for cloning that file on APFS. `ri_logical_writes` was not used:
it is a different ledger and can decrease. These counters cover instrumented process I/O, not
unattributed OS activity, all device traffic or physical NAND writes. Forced termination and
uninstrumented/exec-ending images are not a lossless byte-accounting path; cancellation results
are correctness evidence only. The measured completed builds had no unfinished observed processes.

The signed binary blocks observation libraries. I/O used an ad-hoc-signed copy with all 15
Mach-O payload sections unchanged
([verification](./binary-verification.json)); timings and functional checks used the original.
The observer used direct pinned Cargo/rustc and Xcode clang paths so the observed compiler/linker
chain retained instrumentation. A separate ordinary-linker backtrace check passed.

Cache counts use non-overlapping time windows in isolated event logs because automatic roots
varied with external target paths. The reproducer sets `KACHE_EVENT_ROOT` explicitly.

## Repeat workloads in owned temporary state

Create a task worktree and read-only producer at the measured revision under `.worktrees/`, per
[Git workflow](../../standards/git-workflow.md). From the task worktree:

```console
export PLEXMATON_WORKTREE="$PWD"
export PLEXMATON_PRODUCER="<primary>/.worktrees/kache-producer"
export PLEXMATON_KACHE_TRIAL="$(mktemp -d /private/tmp/plexmaton-kache.XXXXXX)"
```

Download the [v0.17.0 release](https://github.com/kunobi-ninja/kache/releases/tag/v0.17.0) archive
`kache-aarch64-apple-darwin.tar.gz` into the stub. Verify SHA-256
`8bb4140f6bbe2a69f75c7ac1c6d1746eab2215c58bdfb29931fb9d214823cb24` and extract into `tool/`.
Create `kache.toml` there:

```toml
[cache]
local_only = true
daemon_idle_timeout_secs = 120
```

[trial_support.py](./trial_support.py) supplies absolute store/runtime paths and a 2 GiB
registered-blob threshold. Live shared outputs can prevent reclamation; this is not a total-disk cap.
These manual macOS prototypes refuse existing case directories. The real workload temporarily edits
`theme.rs` and restores it on completion or Python exceptions; use only the owning task's source.

```console
python3 .agents/spikes/kache-trial/fixture_trial.py
python3 .agents/spikes/kache-trial/concurrency_cancel.py
python3 .agents/spikes/kache-trial/real_trial.py
```

The [fixture](./fixture_trial.py) exercises default caching; [concurrency/cancellation](./concurrency_cancel.py)
and [real workload](./real_trial.py) set both guarded options. Compiler paths come from `rustup which` and `xcrun`.

For I/O, compile the observer and calibration into the stub:

```console
xcrun clang -dynamiclib .agents/spikes/kache-trial/io-account.c -o "$PLEXMATON_KACHE_TRIAL/io-account.dylib"
xcrun clang .agents/spikes/kache-trial/io-calibration.c -o "$PLEXMATON_KACHE_TRIAL/io-calibration"
python3 .agents/spikes/kache-trial/trial_support.py
cp "$PLEXMATON_KACHE_TRIAL/tool/kache" "$PLEXMATON_KACHE_TRIAL/tool/kache-metered"
codesign --force --sign - "$PLEXMATON_KACHE_TRIAL/tool/kache-metered"
export PLEXMATON_KACHE_TOOL="$PLEXMATON_KACHE_TRIAL/tool/kache-metered"
python3 .agents/spikes/kache-trial/real_trial.py
```

Metered targets/cache are separate. Fixture reruns need a fresh stub. These scripts repeat workloads;
the backtrace, binary-section and producer-removal checks were manual, and `results.json` was curated
from logs. Keep official-binary timings. Stop the owned daemon, retain reports, retire worktrees
through Git and delete the stub.
