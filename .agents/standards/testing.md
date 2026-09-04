# Standard — Testing

| Field | Value |
| --- | --- |
| Trigger | You are about to write, change, or delete a test |
| Owns | Tier selection, test-double policy, snapshot policy, performance evidence |

Tests are part of the design. Choose the lowest tier that can prove the contract, then add
higher-tier coverage only for boundary behavior the lower tier cannot exercise.

## Tier 0 — static and compile-time gates

Purpose: reject invalid code and inconsistent style quickly.

- Formatting
- Compilation of affected targets/features
- Clippy with project policy
- Documentation/link/schema checks where applicable

These gates are necessary but do not prove runtime behavior.

## Tier 1 — unit tests

Purpose: prove pure/local invariants with fast deterministic feedback.

Good targets include:

- Reducers and state transitions
- Geometry, clipping, z-order, hit testing, and scroll-anchor calculations
- Context projection and provider conversion helpers
- Parsers and stream assembly
- Tool-schema and capability decisions
- Truncation, budgeting, and identity rules

Unit tests should not boot the entire application. Use table-driven tests and property tests where
the input space has meaningful invariants.

## Tier 2 — component tests

Purpose: exercise one owned subsystem through its public boundary with realistic collaborators.

Examples:

- TUI reducer + `SurfaceTree` + Ratatui `TestBackend`
- Transcript virtualization with synthetic semantic events
- Provider adapter with a scripted byte stream
- Tool scheduler with deterministic fake tools and cancellation
- Storage implementation against an isolated temporary store

Prefer small behavioral fakes over mocks of internal methods. Component tests may use an
in-memory/test adapter only when it preserves the production contract and failure modes being
tested.

## Tier 3 — contract and fixture tests

Purpose: prove external protocols and compatibility independently of live services.

- Recorded/sanitized provider streams and request bodies
- Server-Sent Events chunk-boundary matrices, including invalid and partial UTF-8
- MCP protocol fixtures
- Session/store migration and corruption fixtures
- Terminal capability and math-render corpora

Fixtures must be named, minimal, immutable, sanitized, and traceable to the behavior they cover.
Provide builders for variations; do not duplicate large opaque blobs across tests.

## Tier 4 — integration and end-to-end tests

Purpose: prove that real subsystem boundaries compose correctly.

- Runnable binary with deterministic synthetic runtime
- PTY/virtual-terminal interaction where escape-sequence behavior matters
- Full A-to-B multi-agent experience with fake providers/tools but real routing, cancellation, and UI loop
- Process, filesystem, storage, and shutdown behavior in isolated temporary workspaces

Use real first-party components and fake only the external boundary. Assert user-visible and
durable outcomes, not private call order.

## Tier 5 — live, compatibility, and manual validation

Purpose: validate assumptions that cannot be proven hermetically.

- Opt-in live provider smoke tests
- Real terminal/tmux/SSH graphics and input checks
- Platform-specific clipboard and process behavior
- Visual/interaction review of representative workflows

Live tests are never the default correctness gate, never require developer secrets for ordinary
test runs, and never substitute for deterministic coverage.

## Performance and resource tests

Performance evidence is a separate lane, not a timing assertion hidden in ordinary unit tests.

- Define workload, terminal size, build profile, warm/cold state, sample count, and metric.
- Measure input-to-frame, scroll-to-frame, redraw work, layout invalidation, memory, and
  time-to-first-visible-output where applicable.
- Use generous regression budgets tied to product impact; do not assert unstable microsecond
  timings on shared machines.
- Include adversarial scale: long transcripts, several streaming agents, slow consumers,
  cancellation, and resize churn.

## Test doubles

- Fake at architectural boundaries, not between every function.
- Prefer deterministic fakes with scripted state transitions over expectation-heavy mocks.
- Mock clocks, IDs, randomness, network byte streams, and external processes when determinism
  requires it.
- Do not use arbitrary sleeps to coordinate tests. Use events, barriers, paused/mock time, or
  explicit readiness signals.
- Inject failures intentionally: partial writes, malformed events, cancellation at boundaries,
  queue saturation, storage errors, and terminal resize.
- A test must fail for a plausible bug. Before keeping it, be able to name the regression it
  detects, and prefer proving it by mutating the implementation and watching that test fail.
- Do not duplicate the same assertion at every tier; each tier should add distinct confidence.
- Flaky tests are bugs. Fix the synchronization or contract; do not add retries until they turn
  green.

## Snapshots and golden files

- Snapshot semantic or cell-buffer output only when a reviewer can understand the diff.
- A full-screen frame proves a composition. When a family of frames differs by one row, that
  row is the claim and a structural assertion is the instrument: the surrounding chrome is
  already frozen by the composition's own frame, and copying it per variant freezes the same
  cells many times so that one layout change rewrites all of them. Crop to the region under
  test, or assert on the drawn text and keep no fixture.
- Normalize unstable IDs, timestamps, paths, and terminal-dependent values at the fixture boundary.
- Pair important snapshots with structural assertions so an empty or truncated snapshot cannot pass
  unnoticed.
- Review snapshot updates as behavior changes; never bulk-accept them without inspection.

## Evidence tooling

A test dependency enters a manifest when the test that needs it is written, not when it is planned.
Versions are audited at that moment and are not recorded here: a version audited months before its
first use is a stale answer wearing a precise number.

| Crate | Role |
| --- | --- |
| `proptest` | Invariants stated over generated inputs. Entered with selection, not clipping: what earned it is that copying must return the same characters at every width and scroll position, which is a claim over a space rather than over cases anyone would enumerate |

Use Ratatui's `TestBackend` before adding a virtual-terminal dependency. Add pseudo-terminal or
virtual-terminal emulation only when a test needs escape-sequence behavior the cell buffer cannot
represent.
