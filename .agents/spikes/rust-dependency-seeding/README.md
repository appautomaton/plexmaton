# Dependency reuse evaluation

| Field | Value |
| --- | --- |
| Read when | Reconsidering compiled dependency reuse across task worktrees |
| Question | Does selective APFS seeding provide a stable benefit worth maintaining? |
| Decision | Use ordinary Cargo with private task targets; no project seeder |

On 2026-09-07, source `1393bcb`, Cargo 1.98 and macOS/APFS, the conservative experiment
selected 47 external packages / 48 library units and cloned 78.69 MiB of dependency data.
The workload was `cargo test -p plexmaton-tui --lib --no-run --offline --locked` with six jobs.

| Condition | Elapsed | Observed process writes |
| --- | ---: | ---: |
| Empty target after host-cache warmup | 10.594 s | 622.48 MiB |
| Seed preparation | 0.403 s | 0.51 MiB |
| Seeded build after donor removal | 9.892 s | 493.12 MiB |

Including preparation, seeding took 10.295 s. No material wall-time improvement was
established. Observed process writes were about 21% lower in these individual samples;
host activity was uncontrolled and child-process accounting was not proven complete.
These are not measurements of unique physical storage, device traffic or NAND writes.

Rejected: maintaining a project-specific dependency seeder, because the limited resource
signal and unproven repeatable time benefit do not justify Cargo-layout compatibility,
artifact admission, locking and publication maintenance. Passing correctness tests does
not establish a positive long-term maintenance tradeoff. Reconsider only with repeatable
benefits and a simpler, supported reuse boundary.

The [archived measurements](https://github.com/appautomaton/plexmaton/blob/bec9b9bca02000768af91361101cf1ef0aa62db7/.agents/spikes/rust-dependency-seeding/command-results.json)
and implementation remain in Git history. [Rust builds](../../standards/rust-builds.md)
owns the current build workflow.
