# Standard — Rust code and dependencies

| Field | Value |
| --- | --- |
| Trigger | You are about to organize a module, choose a style, or add, upgrade, or remove a dependency |
| Owns | Toolchain policy, module organization, error and safety rules, dependency admission |

## Toolchain

- Rust edition and toolchain are defined and exactly pinned at the workspace root. Use the
  project-local toolchain; do not modify the user's global rustup default.
- This binary project may adopt newer stable Rust releases when they provide a concrete
  language/compiler benefit, fix a relevant defect or advisory, or are required by a selected
  dependency. Do not preserve an old Minimum Supported Rust Version (MSRV) without a declared
  distribution need.
- Treat a toolchain bump as a reviewed project change: record the reason, update the pin and
  declared `rust-version`, inspect new compiler/Clippy findings, and run the full applicable
  quality lanes.

## Organization and style

- Prefer small cohesive modules with explicit public boundaries. Split by responsibility and
  invariant, not by arbitrary line counts.
- Keep first-party APIs narrow. Public types and functions need useful documentation about
  contracts and invariants, not restated signatures.
- Comments explain why a constraint exists, which failure it prevents, or why an alternative was
  rejected.
- Use `rustfmt`; do not hand-format against it.
- Treat Clippy findings as design feedback. Suppress narrowly, with a reason, rather than adding
  broad crate-level allowances.
- Preserve deterministic ordering when it affects rendering, serialization, snapshots, or
  user-visible behavior.
- Do not perform speculative micro-optimization. Do avoid obvious whole-history cloning, repeated
  parsing, unbounded allocation, and blocking work; measure before adding complex fast paths.

## Errors and safety

- Core/library code uses typed errors (`thiserror`); application composition may add context with
  `anyhow`.
- Avoid `unwrap`/`expect` on user input, terminal behavior, network, storage, provider data, or
  external processes. An invariant-only `expect` must state the invariant it proves.
- First-party `unsafe` is forbidden by default. Any exception requires an isolated module, explicit
  safety invariants, dedicated tests, and user approval.
- Error strings are never machine-readable control flow.

## Dependency admission

- Add a dependency only for an active-phase capability or measurement.
- Before adding one, inspect maintenance status, license, Minimum Supported Rust Version (MSRV),
  features, native/system requirements, and foundational-version compatibility.
- Declare common versions in `[workspace.dependencies]` and inherit them in member crates.
- Use an audited concrete version as the manifest baseline and commit the exact resolved graph in
  `Cargo.lock`; do not use wildcard (`*`) requirements or unreviewed `latest` aliases.
- Commit `Cargo.lock` because Plexmaton ships binaries.
- Re-run the active phase's dependency audit before initial resolution and at deliberate upgrade
  points. Review changelogs and `cargo tree` output; a numerically newer release is not
  automatically the right release. Use `cargo tree -d` and `cargo tree -e features`.
- Disable default features when they pull unused backends, formats, runtimes, or native libraries.
- Avoid duplicate incompatible generations of foundational crates such as Ratatui or Crossterm.
  `cargo deny` fails the build on this.
- Keep optional/native-heavy integrations behind narrow features and adapters. Keep experimental
  dependencies out of semantic core types and public contracts.
- Prefer adapters we own at external boundaries: clipboard, math rendering, provider transport,
  storage, terminal graphics, and plugins. Write the seam before adding the crate, not after.
- Do not downgrade a locked foundational dependency to accommodate an experiment.
- Remove unused dependencies as part of the change that makes them unused.

## Audited foundation

Versions and features are declared once, in `[workspace.dependencies]`. This table owns only what a
manifest cannot express: why each crate is here and what its feature set is allowed to become.

Audited 2026-09-03 against the graph resolved in `Cargo.lock`.

| Crate | Role | Feature and version decision |
| --- | --- | --- |
| `ratatui` | Cell buffer, layout, text, widgets, test backend | Current modular generation and umbrella crate; splitting its subcrates requires a measured compile-time or boundary benefit |
| `reqwest` | Pooled streaming HTTP client | Defaults off; `json`, `stream`, Rustls. Redirects are disabled around bearer authority. Rustls selects vendored `aws-lc-rs`, not platform OpenSSL |
| `rustix` | Descriptor-relative file access and Unix process-group signalling | Defaults off; `fs`, `process`, `std`. Pinned-toolchain compatible; Apache-2.0 WITH LLVM-exception / Apache-2.0 / MIT; no system library. Unix only |
| `crossterm` | Terminal lifecycle and input events | `event-stream`, and one event-reader path. `osc52` arrived with `plexmaton-cli::clipboard`, which is the only caller; it brings `base64` and nothing else |
| `eventsource-stream2` | Incremental SSE framing at the provider boundary | Maintained fork with partial-chunk and UTF-8 handling; `std` only. It frames events and knows no provider JSON |
| `tokio` | Async task and event runtime | Direct defaults off; Plexmaton enables `rt`, `macros`, `sync`, `time`, `io-util`, and `process`, while reqwest's resolved HTTP graph additionally enables `fs` and `net`. Never `full`; `rt-multi-thread` and `signal` wait for an owner |
| `tokio-util` | Hierarchical cancellation | Defaults are empty; `rt` only, for `CancellationToken` and child tokens |
| `futures-util` | Stream combinators | The focused crate, not the `futures` umbrella; only the features `StreamExt` and the synthetic streams need |
| `serde` / `serde_json` | Deterministic scenario, snapshot and durable-journal data | `derive` enabled; identity and opaque-replay decoding still pass through validating constructors |
| `toml` | Typed user configuration | Parser and Serde only; no formatting/preserve-order surface and no generic configuration framework |
| `thiserror` | Library error types | No `anyhow::Error` in core contracts |
| `anyhow` | Composition-root errors | Binary boundary only |
| `tracing` / `tracing-subscriber` | Structured diagnostics | Only the formatting and filtering layers in use; logs are redirected away from the owned screen |
| `unicode-width` | Terminal-cell measurement | Load-bearing for layout and hit-test correctness; keep the CJK behaviour explicit and tested |
| `unicode-segmentation` | Grapheme-aware editing and selection | Never index visible text by byte offset |
| `proptest` | Property tests, `dev-dependencies` only | Defaults off: `fork` and `timeout` isolate a failing case in a subprocess, which pulls `rusty-fork` and `tempfile` for nothing these properties need |
| `uuid` | Collision-safe automatic session identity | Defaults off; `std` and UUIDv7 only. Journal chronology remains the separately injected `created_at_unix_ms` |

### Considered and not adopted

| Crate | Why not |
| --- | --- |
| `ratatui-textarea` | Consumes `crossterm::event::Event`, and exactly one component may (INV-1) |
| `arboard` | A local clipboard reaches the wrong machine over SSH; OSC 52 reaches the user's terminal |
| `criterion` | Cannot see work counts, which are the evidence; timings are reported, never asserted (FR-4) |

## Maintenance

- Keep one supported path for each behavior. Migrations must have a bounded start, cutover
  criterion, and removal step.
- Refactor when a real responsibility boundary becomes visible; do not postpone obvious state
  duplication or ownership confusion under the label of future cleanup.
- Preserve backward compatibility only when the project explicitly declares a public contract that
  requires it.
- New extension points begin as narrow internal seams. Generalize them after at least two real
  integrations demonstrate the shared contract.
