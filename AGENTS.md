# AGENTS.md — Plexmaton

This file defines project-level rules for work in `plexmaton`. It inherits the workspace rules from `../AGENTS.md`; the stricter applicable rule wins.

Plexmaton is a Rust agentic harness with a responsive Ratatui interface, explicit session/context state, multiple provider dialects, durable tools and sessions, and asynchronous multi-agent collaboration. It is early-stage: preserve the design intent, but do not manufacture compatibility or abstraction for users and APIs that do not yet exist.

## Instruction and context routing

Keep context high-signal. Read only the material needed for the active task.

Default routing order:

1. This `AGENTS.md`.
2. `.agents/DECISIONS.md` when you need to know whether something is already settled, what was rejected, or why. It is an index; follow its links rather than treating it as the rule.
3. `.agents/roadmap/plexmaton.md` for durable product and architecture direction.
4. The active phase linked by that roadmap.
5. `.agents/specs/<mechanism>.md` before implementing or reviewing that mechanism. Specs carry numbered invariants and an evidence table; cite the invariant in the test that proves it.
6. Only the relevant sections of `.agents/roadmap/ui-ux.md` for UI/UX work.
7. The nearest source, tests, and module documentation for the code being changed.
8. Third-party references under `.references/` only when a concrete comparison is needed. That directory is gitignored and is not present in a fresh clone; treat its absence as normal and do not reconstruct it to answer a question.

If `.agents/handoffs/` happens to contain a letter, it is an orientation note the user asked a previous session to leave. Reading the newest one is optional and costs little when picking up unfamiliar work. It routes and warns; it never holds a rule, so it is never a substitute for anything above.

Do not load every roadmap phase, all reference repositories, or broad source trees by default. Progressive disclosure is a working rule, not just a documentation style.

When documenting work:

- Keep `plexmaton.md` short and durable.
- Put cross-cutting interaction rules in `ui-ux.md`.
- Put the precise, testable definition of one mechanism in `specs/`, following the shape in `specs/README.md`. A spec invariant with no test is marked unproven, never left reading as fact.
- Add a row to `DECISIONS.md` when something is actually decided, and record the rejected alternative when one was seriously considered. Never let that file become the only statement of a rule.
- Put implementation scope, evidence, and exit criteria in the active phase file.
- Expand future phase documents just in time, using evidence from the current phase.
- Link to one source of truth instead of copying the same rule into several files.
- Promote a finding to the parent roadmap only when it changes a durable invariant or system boundary.
- Prefer decisions, constraints, evidence, and unresolved questions over narrative history.

## Core engineering principles

### Explicit state and ownership

- Maintain one authoritative representation of session state. Transient turn assembly may exist, but it must not become a second transcript requiring later reconciliation.
- Make lifecycle states explicit with enums/state machines. Avoid collections of loosely related booleans.
- Use stable typed identities for agents, sessions, turns, transcript items, tool calls, mail, artifacts, and surfaces.
- Keep semantic source separate from rendered presentation and provider-specific replay metadata.
- Every background task must have an owner, cancellation path, bounded resources, and observable completion. Do not detach anonymous tasks.
- Use bounded channels and queues unless an unbounded structure is justified by a proven hard upper bound.
- Cancellation, timeout, retry, partial completion, and shutdown are normal state transitions, not exceptional afterthoughts.
- Avoid mutable process-global state. If process-wide coordination is required, give it an explicit owner and testable lifecycle.

### Architectural direction

- Dependencies point inward: adapters and UI depend on semantic core contracts, never the reverse.
- The TUI consumes revisioned semantic events/snapshots and emits intents. Widgets must not call providers, storage, tools, or agent objects directly.
- Rendering is a projection of state. Do not hide business transitions inside `render`/`view` functions.
- Keep terminal event routing, focus, pointer capture, z-order, clipping, and scroll ownership centralized in the interaction/surface layer.
- Provider APIs are wire adapters over a shared semantic core; Chat Completions, Responses, and Messages do not get separate agent loops.
- Tools expose typed schemas, effects/capabilities, and bounded outputs. Tool names or prompt prose are not security boundaries.
- Agent-to-agent mail is a typed domain concept, not a fake user message or a blocking delegation result.
- Preserve exact semantic content for selection/copy. Do not reconstruct copied content from decorated or clipped terminal cells.

### Abstraction discipline

Use enough abstraction to protect a real boundary and enable genuine reuse. Do not optimize for hypothetical reuse.

Create an abstraction when at least one is true:

- Two real callers share the same invariant and behavior.
- An external boundary needs a replaceable/testable adapter.
- A type prevents invalid states or enforces ownership/capability rules.
- A measured hot path needs an isolated implementation strategy.

Do not create an abstraction merely because:

- A second implementation might exist someday.
- A function is long but still has one coherent responsibility.
- A design pattern has a familiar name.
- A wrapper can hide an inconvenient API without improving the domain model.

Prefer a concrete implementation with a narrow seam over a general framework. It is easier to extract a correct abstraction from two working cases than to remove a speculative one embedded throughout the system.

## Anti-patterns to avoid

The following require redesign, not explanation:

- God crates, God structs, and catch-all modules that own unrelated runtime, UI, provider, and persistence concerns.
- One universal event/message enum mixing provider wire events, durable events, UI animation, terminal input, and agent mail.
- Stringly typed states, roles, error categories, capabilities, IDs, or routing decisions.
- Multiple sources of truth with synchronization code between them.
- Hidden mutable state in globals, thread-locals, render caches, or callbacks.
- UI components that mutate runtime/session state during rendering.
- Blocking filesystem, network, model, tool, math-render, or persistence work on the TUI event loop.
- Re-layout or cloning of full transcripts for each streaming delta.
- Unbounded tool output, transcript injection, queues, retries, task spawning, or in-memory caches.
- Fire-and-forget tasks whose errors and shutdown are discarded.
- Silent fallback that changes semantics or security posture. Degradation must be typed, visible, and testable.
- Compatibility shims, deprecated parallel paths, `foo_v2`, or simultaneous replacement implementations without a bounded migration plan.
- Generic `Manager`, `Service`, `Context`, or `Utils` buckets with unclear ownership.
- Error strings used as machine-readable control flow.
- Premature custom HTTP, database, scheduler, widget, or plugin frameworks when maintained crates satisfy the measured need.
- Feature flags that create many untested product combinations. Features should isolate optional transports/integrations, not fork core semantics.
- Large snapshots that reviewers cannot meaningfully inspect.
- Tests that only confirm mocks returned what they were configured to return.

## Code organization and style

- Rust edition and toolchain are defined and exactly pinned at the workspace root. Use the project-local toolchain; do not modify the user's global rustup default.
- This binary project may adopt newer stable Rust releases when they provide a concrete language/compiler benefit, fix a relevant defect or advisory, or are required by a selected dependency. Do not preserve an old Minimum Supported Rust Version (MSRV) without a declared distribution need.
- Treat a toolchain bump as a reviewed project change: record the reason, update the pin and declared `rust-version`, inspect new compiler/Clippy findings, and run the full applicable quality lanes.
- Declare common dependency versions in `[workspace.dependencies]` and inherit them in member crates.
- Commit `Cargo.lock` because Plexmaton ships binaries.
- Keep optional/native-heavy integrations behind narrow features and adapters.
- Avoid duplicate incompatible generations of foundational crates such as Ratatui or Crossterm.
- Prefer small cohesive modules with explicit public boundaries. Split by responsibility and invariant, not by arbitrary line counts.
- Keep first-party APIs narrow. Public types and functions need useful documentation about contracts and invariants, not restated signatures.
- Comments explain why a constraint exists, which failure it prevents, or why an alternative was rejected.
- Use `rustfmt`; do not hand-format against it.
- Treat Clippy findings as design feedback. Suppress narrowly, with a reason, rather than adding broad crate-level allowances.
- Avoid `unwrap`/`expect` on user input, terminal behavior, network, storage, provider data, or external processes. An invariant-only `expect` must state the invariant it proves.
- First-party `unsafe` is forbidden by default. Any exception requires an isolated module, explicit safety invariants, dedicated tests, and user approval.
- Core/library code uses typed errors (`thiserror`); application composition may add context with `anyhow`.
- Preserve deterministic ordering when it affects rendering, serialization, snapshots, or user-visible behavior.
- Do not perform speculative micro-optimization. Do avoid obvious whole-history cloning, repeated parsing, unbounded allocation, and blocking work; measure before adding complex fast paths.

## Dependency discipline

- Add a dependency only for an active-phase capability or measurement.
- Before adding one, inspect maintenance status, license, Minimum Supported Rust Version (MSRV), features, native/system requirements, and foundational-version compatibility.
- Use an audited concrete version as the manifest baseline and commit the exact resolved graph in `Cargo.lock`; do not use wildcard (`*`) version requirements or unreviewed `latest` aliases.
- Re-run the active phase's dependency audit before initial resolution and at deliberate upgrade points. Review changelogs and `cargo tree` output; a numerically newer release is not automatically the right release.
- Disable default features when they pull unused backends, formats, runtimes, or native libraries.
- Keep experimental dependencies out of semantic core types and public contracts.
- Prefer adapters we own at external boundaries: clipboard, math rendering, provider transport, storage, terminal graphics, and plugins.
- Do not downgrade a locked foundational dependency to accommodate an experiment.
- Remove unused dependencies as part of the change that makes them unused.

## Testing policy: meaningful and properly tiered

Tests are part of the design. Choose the lowest tier that can prove the contract, then add higher-tier coverage only for boundary behavior the lower tier cannot exercise.

### Tier 0 — static and compile-time gates

Purpose: reject invalid code and inconsistent style quickly.

- Formatting
- Compilation of affected targets/features
- Clippy with project policy
- Documentation/link/schema checks where applicable

These gates are necessary but do not prove runtime behavior.

### Tier 1 — unit tests

Purpose: prove pure/local invariants with fast deterministic feedback.

Good targets include:

- Reducers and state transitions
- Geometry, clipping, z-order, hit testing, and scroll-anchor calculations
- Context projection and provider conversion helpers
- Parsers and stream assembly
- Tool-schema and capability decisions
- Truncation, budgeting, and identity rules

Unit tests should not boot the entire application. Use table-driven tests and property tests where the input space has meaningful invariants.

### Tier 2 — component tests

Purpose: exercise one owned subsystem through its public boundary with realistic collaborators.

Examples:

- TUI reducer + `SurfaceTree` + Ratatui `TestBackend`
- Transcript virtualization with synthetic semantic events
- Provider adapter with a scripted byte stream
- Tool scheduler with deterministic fake tools and cancellation
- Storage implementation against an isolated temporary store

Prefer small behavioral fakes over mocks of internal methods. Component tests may use an in-memory/test adapter only when it preserves the production contract and failure modes being tested.

### Tier 3 — contract and fixture tests

Purpose: prove external protocols and compatibility independently of live services.

- Recorded/sanitized provider streams and request bodies
- Server-Sent Events chunk-boundary matrices, including invalid and partial UTF-8
- MCP protocol fixtures
- Session/store migration and corruption fixtures
- Terminal capability and math-render corpora

Fixtures must be named, minimal, immutable, sanitized, and traceable to the behavior they cover. Provide builders for variations; do not duplicate large opaque blobs across tests.

### Tier 4 — integration and end-to-end tests

Purpose: prove that real subsystem boundaries compose correctly.

- Runnable binary with deterministic synthetic runtime
- PTY/virtual-terminal interaction where escape-sequence behavior matters
- Full A-to-B multi-agent experience with fake providers/tools but real routing, cancellation, and UI loop
- Process, filesystem, storage, and shutdown behavior in isolated temporary workspaces

Use real first-party components and fake only the external boundary. Assert user-visible and durable outcomes, not private call order.

### Tier 5 — live, compatibility, and manual validation

Purpose: validate assumptions that cannot be proven hermetically.

- Opt-in live provider smoke tests
- Real terminal/tmux/SSH graphics and input checks
- Platform-specific clipboard and process behavior
- Visual/interaction review of representative workflows

Live tests are never the default correctness gate, never require developer secrets for ordinary test runs, and never substitute for deterministic coverage.

### Performance and resource tests

Performance evidence is a separate lane, not a timing assertion hidden in ordinary unit tests.

- Define workload, terminal size, build profile, warm/cold state, sample count, and metric.
- Measure input-to-frame, scroll-to-frame, redraw work, layout invalidation, memory, and time-to-first-visible-output where applicable.
- Use generous regression budgets tied to product impact; do not assert unstable microsecond timings on shared machines.
- Include adversarial scale: long transcripts, several streaming agents, slow consumers, cancellation, and resize churn.

### Test-double rules

- Fake at architectural boundaries, not between every function.
- Prefer deterministic fakes with scripted state transitions over expectation-heavy mocks.
- Mock clocks, IDs, randomness, network byte streams, and external processes when determinism requires it.
- Do not use arbitrary sleeps to coordinate tests. Use events, barriers, paused/mock time, or explicit readiness signals.
- Inject failures intentionally: partial writes, malformed events, cancellation at boundaries, queue saturation, storage errors, and terminal resize.
- A test must fail for a plausible bug. Before keeping it, be able to name the regression it detects.
- Do not duplicate the same assertion at every tier; each tier should add distinct confidence.
- Flaky tests are bugs. Fix the synchronization or contract; do not add retries until they turn green.

### Snapshot and golden-file rules

- Snapshot semantic or cell-buffer output only when a reviewer can understand the diff.
- Normalize unstable IDs, timestamps, paths, and terminal-dependent values at the fixture boundary.
- Pair important snapshots with structural assertions so an empty or truncated snapshot cannot pass unnoticed.
- Review snapshot updates as behavior changes; never bulk-accept them without inspection.

## Quality workflow

Before changing code:

- Read the routed roadmap/phase material and nearest module tests.
- Check repository status and preserve unrelated user changes.
- Identify the contract and the lowest meaningful test tier.

While changing code:

- Keep patches scoped to one coherent outcome.
- Add or update the test that proves the behavior, including relevant failure/cancellation paths.
- Run the smallest relevant test first, then the broader owning-crate/workspace gate proportional to risk.
- Do not claim a check passed unless it was actually run in this workspace.

Before handoff:

- Inspect the diff for accidental dependency, generated-file, snapshot, or formatting churn.
- Report what changed, what was tested, and what remains unverified.
- Update active-phase evidence only when the exit criterion is actually demonstrated.

The current workspace gates are:

- `cargo fmt --all --check`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

Plus the supply-chain lane, which depends on the resolved graph rather than on any single edit:

- `cargo deny check` — licences, advisories, and the ban on duplicate Ratatui/Crossterm generations
- `cargo machete` — dependencies that are declared but unused
- `typos` — prose and identifier spelling
- `./scripts/check-file-length.sh` — module sprawl sentinel

Formatting and lint policy are pinned in `rustfmt.toml`, `clippy.toml`, `deny.toml`, `_typos.toml`, and the root `[workspace.lints]` table.

Sprawl guards exist at two levels and are thresholds for finding unseparated responsibilities, not line-count style rules. `too_many_lines` and `cognitive_complexity` work at function level and are the effective guard, because a large file of small functions is usually fine while a long function never is. `check-file-length.sh` adds a 400-line file-level sentinel measured above the first `#[cfg(test)]` module, so inline tests do not count against the budget. When either fires, split by responsibility and invariant; raising the threshold is not the fix.

Local commits run the fast gates through a repository-managed hook. Enable it once per clone:

```console
git config core.hooksPath .githooks
```

`./scripts/smoke-tui.py` covers terminal lifecycle that `TestBackend` cannot represent: alternate-screen release, resize repaint, and the quit key in front of a real pseudo-terminal. It runs in CI and should be run locally when changing the event loop, terminal setup, or layout classes.

Use `cargo tree -d` and `cargo tree -e features` when adding or upgrading dependencies. Add narrower crate/test commands as the owning modules become substantial; do not replace the workspace gates with undocumented local variants.

## Maintenance and evolution

- Keep one supported path for each behavior. Migrations must have a bounded start, cutover criterion, and removal step.
- Refactor when a real responsibility boundary becomes visible; do not postpone obvious state duplication or ownership confusion under the label of future cleanup.
- Preserve backward compatibility only when the project explicitly declares a public contract that requires it.
- New extension points begin as narrow internal seams. Generalize them after at least two real integrations demonstrate the shared contract.
- Keep roadmap, code, tests, and user-facing behavior aligned. Stale comments and contradictory feature defaults are defects.
- Do not commit, publish, install globally, or mutate live user configuration unless the user explicitly requests it.
