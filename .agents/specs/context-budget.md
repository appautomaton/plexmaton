# Spec — Context budget

| Field | Value |
| --- | --- |
| Status | Implemented; automatic compaction follows CPL-7 |
| Owns | Pure request-occupancy estimates, exact input-usage anchors and budget decisions |
| Depends on | JRN-5, TIM-3/TIM-4, PRV-3/PRV-6 |
| Proven by | Agent arithmetic/journal tests, provider codec estimates and runtime snapshot tests below |

## Invariants

**BUD-1 — The ledger is a projection.** One selected journal path and immutable request environment
produce one ledger without a write, model call, head mutation or stored parallel history. Its
snapshot contains counts and identities, never prompt text or opaque replay.

**BUD-2 — A measurement covers exactly its input.** Only reported agent-step input with the same
request environment, context epoch and exact ordered atom prefix on this path may anchor a ledger. The longest
matching prefix wins, with the latest authorization breaking ties; unavailable usage,
compaction requests and sibling paths cannot supply a measurement.
An individual request's missing optional breakdown does not invalidate its measured input;
aggregated partial counts never supply an anchor.
Rejected: using billed `total` or generated `output` as measured input occupancy, and subtracting
cache hits from occupied context.

**BUD-3 — Estimates retain their uncertainty.** Each whole atom and the request environment receive
a deterministic `utf8_heuristic_v1` estimate: serialized UTF-8 bytes divided by four, rounded up per
unit. Opaque replay bytes contribute only a flagged byte-size heuristic, never an exact token count
or an upper-bound promise; compatible encoding is validated before budgeting.

**BUD-4 — Limits and arithmetic are explicit.** Measured prefix plus estimated suffix (or the full
estimate without an anchor) is input occupancy; output reserve is separate. `Fits` includes the
soft boundary, aggregate pressure is `CompactionNeeded`, and an indivisible atom plus environment
that exceeds input capacity is `ImpossibleItem`. Arithmetic overflow and invalid limits are typed
errors; no wrapped count or missing measurement appears as zero.

## Model

The environment is included once: in the measurement when anchored, otherwise in the estimate.
The current [AGI-4 instruction snapshot](./agent-instructions.md) is part of that environment.
The configured context window minus output reserve is input capacity; the default soft limit is
80% of that capacity. Maximum output remains a separate provider setting (PRV-6).
Per-atom estimates remain available for later compaction planning even under a measured prefix;
they do not partition the provider's measured count. A measured current input needs no estimated
suffix; a heuristic `Fits` is not a guarantee that a provider will accept the request.

The status-line snapshot projects this ledger alongside TIM-3 accounting. It does not parse
JSONL or calculate an independent budget. CPL-7 owns automatic compaction and dispatch gating.
`LiveRuntime::context_budget` exposes acknowledged facts on demand, not per frame. Pending writes,
failed persistence, incomplete tool batches and synthetic drivers without model configuration are
explicit unavailable states; other projection/encoding failures remain typed errors.

## Evidence

[Named proofs](../evidence/context-budget.md), one row an invariant.
