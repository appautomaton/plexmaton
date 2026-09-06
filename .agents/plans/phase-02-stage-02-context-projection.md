# Plan — Phase 02 stage 2, context projection and rewind

| Field | Value |
| --- | --- |
| Phase | [Phase 02 — Durable sessions and context](../phases/phase-02-durable-sessions.md) §scope 1–3 |
| Contract | TIM-1–TIM-5, JRN-1/JRN-3/JRN-5/JRN-7, PRV-1/PRV-3–PRV-6, LIVE-1/LIVE-3–LIVE-5 and LOOP-2 |
| Status | Active; slices 1–9 complete; slice 10 pending |
| Blocked | None for compaction; slice 10 interaction copy requires the user's rendered-frame agreement |

## Outcome

One journal head projects bounded, tool-safe model context. Usage anchors its known prefix; estimates
cover the rest. Compaction preserves source history and starts a branch-local cache epoch.
Named heads preserve independent context bases without copying entries or replaying effects.

## Constraints established before implementation

- The journal remains authority; atoms, ledgers and encoded requests are projections, while a
  compacted replacement is a journaled checkpoint.
- JRN-3 owns the current schema epoch; compaction is additive within that epoch.
- One complete parallel tool call/result batch is an indivisible atom. No budget, suffix, rewind or
  compaction cut splits it; incomplete batches follow JRN-5.
- PRV-3 owns exact block-anchored replay and typed compatibility; compaction never mutates opaque
  payloads or removes them from source history.
- Reported input anchors require the exact atom prefix and environment fingerprint: resolved model,
  instructions and tool definitions. Runtime prompt inputs are budgeted but not copied into JSONL.
- Equal path, checkpoint and environment encode byte-identically. Compaction appends one
  stable instruction after the complete prior input; overflow never rewrites that input. Selection and
  rewind follow [context epochs and branch selection](../ui-ux.md#context-epochs-and-branch-selection).
- The ledger combines the longest matching provider input measurement with estimates for later atoms and a
  separate output reserve. Soft policy and the provider hard limit remain distinct.
- PRV-6 owns the immutable resolved model; BUD-3 owns estimator provenance.
- A checkpoint records source revision, covered identities, replacement context, compatibility and
  cache epoch; originals remain. Malformed structure fails closed, while runtime incompatibility
  blocks only context projection.
- Compaction/rewind planners perform no effect. Journal acknowledgement precedes head selection or
  publishing a replacement.

## Slices

1. **Turn chronology (complete).** Implement TIM-1 with atomic user/turn start and typed terminal
   audit. *Closes when* queue/recovery preserve IDs/times without idle writes, replay clocks or head
   advances, and invalid partial-turn/terminal mutations change nothing.
2. **Schema epoch and session metadata (complete).** Replace prototype numeric headers with the one
   JRN-3 grammar and retain conversation creation time in the in-memory journal. Delete old readers
   and fixtures. *Closes when* create/reopen preserve metadata, a foreign epoch fails before record
   decoding, both production constructors use external time, and idle writes nothing.
3. **Context atoms and replay compatibility (complete).** Project one selected head into typed message,
   reasoning/replay and complete tool-batch atoms. Add the compatibility value that decides whether
   opaque replay may enter a request. *Closes when* live/reloaded heads produce equal ordered atoms,
   head changes cannot split a parallel batch, incompatibility is typed, and both codecs preserve
   compatible ordering.
4. **Resolved model registry (complete).** Split provider routes from named models; remove `kind`/`protocol`
   and the user-facing retained-byte limit. *Closes when* one route supplies two exact models
   without repeated authority, resolved defaults are explicit, invalid input fails before network
   work, old fields are rejected, and README/config fixtures agree.
5. **Request attempts and immutable usage (complete).** Implement TIM-2–TIM-5 with agent-step and compaction
   owners over an exact atom boundary/fingerprint; retire cumulative journal usage. *Closes when*
   cancellation/encoding failure, every dispatched terminal, missing usage and process death retain
   correlation without fabricated timing or delta writes; invalid terminals mutate nothing.
6. **Budget ledger (complete).** Give every atom and request-environment input a deterministic estimated cost;
   reconcile a provider usage report only with the exact prefix/fingerprint it measured. Return
   typed `Fits`, `CompactionNeeded` or `ImpossibleItem`, keeping reserve and hard limit separate.
   *Closes when* suffixes, changed environment, encrypted replay, maximal tool output and provider
   totals have boundary tests; missing usage is never zero; optional breakdowns do not erase measured input. Evidence: [context-budget](../specs/context-budget.md).
7. **Pure compaction plan (complete).** CPL-1–CPL-3 define deterministic whole-atom cuts,
   exact retained context and an unchanged request prefix. The configurable recent-tail target
   defaults to 20k and affects only checkpoint projection. Codec and planner regressions prove
   byte-preserving input extension, measurement reuse and typed overflow refusal.
8. **Durable checkpoint (complete).** CPL-4–CPL-6 define acknowledged checkpoint provenance,
   ancestry projection and full summarizer audit. Current-epoch fixtures reopen with identical context;
   repeated checkpoints and historical forks reconstruct identical bytes through all four codecs.
   Equal-atom-count reductions, stale commits and mismatched epochs have regressions.
9. **Bounded automatic orchestration (complete).** CPL-7/CPL-8 define
   soft/hard triggers, one typed context-error recovery, at most three separate operations per turn,
   owned cancellation/deadlines and visible failures. Tests cover each writer barrier, including
   cancellation before a refreshed agent request; uncertain persistence freezes continuation.
10. **Manageable heads and rewind journey.** Expose create, select, rename, abandon and rewind over
   the existing revision-checked head mutations, with current head and cache-break reason visible.
   Each rewind creates and selects a fresh head at a stable target, preserving the original branch.
   A user-item rewind forks before its turn and returns its text as draft. *Closes when* forks and selection preserve exact requests,
   exit before resubmission creates no recovery, and three widths are reviewed.

## Order, and why

Chronology fixes lifecycle boundaries; the epoch fixes metadata before atoms supply attempt identity.
The resolved model freezes the environment vocabulary before attempts fingerprint it. The ledger
measures pressure before compaction freezes a payload. Checkpoints precede automatic triggers, so
recovery exists first. Rewind composes the earlier boundaries and needs user review.

Rejected: destructive transcript rewrite, a second chat-history authority, index-based tool cuts,
provider response IDs as recovery state, old-schema migration paths, dual canonical payloads, and
unbounded persistence or compaction queues.

## Deliberately not in this plan

Physical garbage collection, cross-session mail, durable permission grants, MCP, additional
provider transports, slash-command infrastructure, animation, themes and layout configuration.
