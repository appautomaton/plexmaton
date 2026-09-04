# Plan — Phase 02 stage 2, context projection and rewind

| Field | Value |
| --- | --- |
| Phase | [Phase 02 — Durable sessions and context](../phases/phase-02-durable-sessions.md) §scope 1–3 |
| Contract | TIM-1–TIM-5, JRN-1/JRN-3/JRN-5/JRN-7, PRV-1/PRV-3–PRV-6, LIVE-1/LIVE-3–LIVE-5 and LOOP-2 |
| Status | Active; slices 1–5 complete, slice 6 of 10 next |
| Blocked | None for slices 1–9; slice 10 interaction copy requires the user's rendered-frame agreement |

## Outcome

One journal head projects bounded, tool-safe model context. Usage anchors its known prefix; estimates
cover the rest. Compaction preserves source history and starts a cache epoch. Named heads make
rewind and branches durable without copying entries or replaying effects.

## Constraints established before implementation

- The journal remains authority; atoms, ledgers and encoded requests are projections, while a
  compacted replacement is a journaled checkpoint.
- Only the `2026-09-04` grammar is supported. Its header carries canonical creation time; numeric
  versions, migration readers and dual payload paths are deleted.
- One complete parallel tool call/result batch is an indivisible atom. No budget, suffix, rewind or
  compaction cut splits it; incomplete batches follow JRN-5.
- Retain every reasoning artifact the model exposes and replay it when its adapter says compatible.
  Opaque replay is block-anchored and requires adapter-owned non-secret scope, codec revision and
  model family; typed incompatibility never translates, merges, drops or truncates ciphertext.
- Usage anchors require the exact atom prefix and environment fingerprint: resolved model,
  instructions and tool definitions. Runtime prompt inputs are budgeted but not copied into JSONL.
- Equal path, checkpoint and environment encode byte-identically. Verbatim compaction appends one
  stable instruction after the prior input; fitted/lossy paths name their cache break. A checkpoint
  starts a cache epoch; rewind selects existing ancestry.
- The ledger combines the last matching provider total with estimates for later atoms and a
  separate output reserve. Soft policy and the provider hard limit remain distinct.
- User TOML nests models under provider routes. Exact selection yields an immutable,
  credential-blind model with typed API, identity, reasoning, token limits, estimator and optional
  cost. Missing cost stays unavailable; a missing estimator resolves to one versioned default.
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
   `2026-09-04` grammar and retain session creation time in the in-memory journal. Delete old readers
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
6. **Budget ledger.** Give every atom and request-environment input a deterministic estimated cost;
   reconcile a provider usage report only with the exact prefix/fingerprint it measured. Return
   typed `Fits`, `CompactionNeeded` or `ImpossibleItem`, keeping reserve and hard limit separate.
   *Closes when* suffixes, changed environment, encrypted replay, maximal tool output and provider
   totals have boundary tests; missing usage is never zero.
7. **Pure compaction plan.** Select a covered prefix and byte-exact retained suffix in atom units;
   build a summarization request as an append-only extension of the old request. Use a bounded
   verbatim → fitted → lossy input ladder and retain source identities plus current user/workspace
   context. *Closes when* planning is deterministic, batches cannot straddle cuts, degradation is
   typed, verbatim input retains the exact provider prefix, and fitted/lossy paths name cache breaks.
8. **Durable checkpoint.** Add one versioned checkpoint payload and projection rule. Commit its
   summary, provenance, source revision, compatibility and cache epoch before selecting the
   replacement view; resume derives checkpoint plus suffix from the same journal. *Closes when*
   deleting projections and reopening yields byte-identical requests, history remains reachable,
   stale commits fail, malformed provenance is typed, and incompatibility blocks only context.
9. **Bounded automatic orchestration.** Invoke compaction before a turn at the soft threshold,
   after a tool result that would cross the hard bound, and once in response to a typed provider
   context error. Own and cancel the summarizer like any model task; cap attempts per turn and keep
   the old head usable on failure. *Closes when* each trigger has one deterministic journey, no
   effect/retry starts before journal acknowledgement, failures are visible, and the next request
   retains the checkpoint epoch's exact input prefix.
10. **Manageable heads and rewind journey.** Expose create, select, rename, abandon and rewind over
   the existing revision-checked head mutations, with current head and cache-break reason visible.
   Rewind to a stable atom boundary without copying or re-executing work. A user-item rewind selects
   its prior stable boundary and returns its text as draft. *Closes when* branches preserve exact requests,
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
