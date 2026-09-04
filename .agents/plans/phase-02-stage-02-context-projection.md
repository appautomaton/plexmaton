# Plan — Phase 02 stage 2, context projection and rewind

| Field | Value |
| --- | --- |
| Phase | [Phase 02 — Durable sessions and context](../phases/phase-02-durable-sessions.md) §scope 2–3 |
| Contract | JRN-1/JRN-3/JRN-5/JRN-7, PRV-1/PRV-3/PRV-4/PRV-5, LIVE-1/LIVE-3/LIVE-4/LIVE-5 and LOOP-2 |
| Status | Ready; slice 1 of 6 |
| Blocked | None for slices 1–5; slice 6 interaction copy requires the user's rendered-frame agreement |

## Outcome

One selected journal head projects a bounded, tool-safe model context. Provider usage anchors the
known prefix; deterministic estimates cover its unmeasured suffix, current instructions, tool
schemas and output reserve. Compaction appends a typed checkpoint while preserving the source
history, and later requests extend one explicit cache epoch. Named heads make rewind and alternate
branches durable and user-manageable without copying entries or replaying effects.

## Constraints established before implementation

- The journal remains authority. Context atoms, budget ledgers, encoded requests and compacted
  views are projections or journaled checkpoints; none becomes a second transcript.
- Projection groups each complete parallel tool call/result batch into one indivisible atom.
  Budgeting, suffix retention, rewind cuts and compaction never split it. An incomplete durable
  batch follows JRN-5 rather than being repaired differently by each provider codec.
- Semantic reasoning and provider replay keep their journal order. Opaque replay is compatible only
  with its recorded codec and model family; an incompatible projector fails typed and never
  translates, merges or truncates ciphertext.
- A usage anchor applies only to the exact request prefix and request-environment fingerprint that
  produced it. The fingerprint covers model/profile, instructions and tool definitions. Those
  runtime prompt inputs are budgeted and cache-relevant but are not copied into session JSONL.
- Equal journal head, checkpoint and request environment produce byte-identical encoded requests.
  The verbatim compaction path appends one stable instruction after the prior ordered provider-input
  sequence, preserving that sequence's cache eligibility. A fitted or lossy path names its cache
  break. The resulting checkpoint begins a named cache epoch; every later non-append projection
  change does the same.
- Provider totals remain evidence, not invented precision. The ledger combines the last matching
  input total with deterministic estimates for later atoms and reserves output separately. Soft
  compaction thresholds are profile policy; the provider's hard context limit is a distinct bound.
- A checkpoint records its source head revision, covered entry identities, replacement semantic
  context, policy/model compatibility and cache epoch. Original entries and exact replay stay in
  the journal. Missing or malformed checkpoint structure fails closed; an incompatible runtime
  refuses context projection while the journal remains loadable and inspectable.
- Compaction and rewind are effectful only at their explicit boundaries. Pure planning performs no
  provider, tool, policy or filesystem work; journal acknowledgement precedes switching a head or
  publishing a replacement projection.

## Slices

1. **Context atoms and replay compatibility.** Project one selected head into typed message,
   reasoning/replay and complete tool-batch atoms. Add the compatibility value that decides whether
   opaque replay may enter a request. *Closes when* live and reloaded heads produce equal ordered
   atoms, every parallel call/result batch is indivisible, incompatible replay is a typed refusal,
   and both provider codecs preserve compatible ordering.
2. **Budget ledger.** Give every atom and request-environment input a deterministic estimated cost;
   reconcile a provider usage report only with the exact prefix/fingerprint it measured. Return
   typed `Fits`, `CompactionNeeded` or `ImpossibleItem` decisions with separate response reserve and
   hard limit. *Closes when* suffix growth, changed tools/instructions, encrypted replay, maximal
   tool output and provider-total replacement have boundary tests without reporting missing usage
   as zero.
3. **Pure compaction plan.** Select a covered prefix and byte-exact retained suffix in atom units;
   build a summarization request as an append-only extension of the old request. Use a bounded
   verbatim → fitted → lossy input ladder and retain source identities plus current user/workspace
   context. *Closes when* repeated planning is deterministic, no tool batch can straddle a cut,
   every degradation is typed, the verbatim path retains the exact ordered provider-input prefix,
   and fitted/lossy paths carry their cache-break cause.
4. **Durable checkpoint.** Add one versioned checkpoint payload and projection rule. Commit its
   summary, provenance, source revision, compatibility and cache epoch before selecting the
   replacement view; resume derives checkpoint plus suffix from the same journal. *Closes when*
   deleting projections and reopening yields byte-identical requests, source history remains
   reachable, stale-head commit is refused, malformed structure or provenance is a typed journal
   projection failure, and runtime model/codec incompatibility refuses only context projection.
5. **Bounded automatic orchestration.** Invoke compaction before a turn at the soft threshold,
   after a tool result that would cross the hard bound, and once in response to a typed provider
   context error. Own and cancel the summarizer like any model task; cap attempts per turn and keep
   the old head usable on failure. *Closes when* each trigger has one deterministic journey, no
   retry loop or effect starts before journal acknowledgement, failures are visible, and the next
   request retains the checkpoint epoch's exact ordered provider-input prefix.
6. **Manageable heads and rewind journey.** Expose create, select, rename, abandon and rewind over
   the existing revision-checked head mutations, with current head and cache-break reason visible.
   Rewind to a stable atom boundary and create an alternate named head without copying history or
   re-executing work. *Closes when* a real compacted session survives branch creation, switching,
   rename, abandonment and reopen; both branches retain their exact provider requests; and the
   agreed interaction is reviewed in wide, medium and narrow frames.

## Order, and why

Atoms define the only safe unit for budgets and cuts. The ledger then supplies measured pressure
without mutating history; the pure plan fixes compaction semantics before a durable payload freezes
them. Checkpoints land before automatic triggers, so recovery exists before background policy can
create them. The user-facing rewind journey comes last because it composes every prior boundary and
needs an interaction decision rather than a speculative command framework.

The reference spike supports this split: Codex contributes raw append-only history, deterministic
prompt normalization and checkpoint-plus-suffix replay; Grok contributes ordered semantic items,
usage-plus-suffix budgeting and bounded compaction degradation. Rejected: destructive transcript
rewrite, a second chat-history authority, index-based tool cuts, provider response IDs as recovery
state, and an unbounded persistence or compaction queue.

## Deliberately not in this plan

Physical garbage collection, cross-session mail, durable permission grants, MCP, additional
provider transports, slash-command infrastructure, animation, themes and layout configuration.
