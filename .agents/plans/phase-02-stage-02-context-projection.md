# Plan — Phase 02 stage 2, context projection and rewind

| Field | Value |
| --- | --- |
| Phase | [Phase 02 — Durable sessions and context](../phases/phase-02-durable-sessions.md) §scope 2–3 |
| Contract | TIM-1–TIM-5, JRN-1/JRN-3/JRN-5/JRN-7, PRV-1/PRV-3/PRV-4/PRV-5, LIVE-1/LIVE-3/LIVE-4/LIVE-5 and LOOP-2 |
| Status | Active; slice 1 complete, slice 2 of 8 active |
| Blocked | None for slices 1–7; slice 8 interaction copy requires the user's rendered-frame agreement |

## Outcome

One journal head projects a bounded, tool-safe model context. Provider usage anchors the known
prefix; estimates cover its suffix, instructions, tools and output reserve. Compaction appends a
checkpoint while preserving source history; later requests extend its cache epoch. Named heads make
rewind and alternate branches durable without copying entries or replaying effects.

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
- Equal semantic path, checkpoint and request environment produce byte-identical encoded requests.
  The verbatim compaction path appends one stable instruction after the prior ordered provider-input
  sequence, preserving that sequence's cache eligibility. A fitted or lossy path names its cache
  break. Each checkpoint begins a named cache epoch; rewind and head selection only select an
  existing ancestry/identity, while any ineligible projection names its typed cause.
- The ledger combines the last matching provider total with estimates for later atoms and a
  separate output reserve. Soft policy and the provider hard limit remain distinct.
- A checkpoint records its source head revision, covered entry identities, replacement semantic
  context, policy/model compatibility and cache epoch. Original entries and exact replay stay in
  the journal. Missing or malformed checkpoint structure fails closed; an incompatible runtime
  refuses context projection while the journal remains loadable and inspectable.
- Compaction and rewind are effectful only at their explicit boundaries. Pure planning performs no
  provider, tool, policy or filesystem work; journal acknowledgement precedes switching a head or
  publishing a replacement projection.

## Slices

1. **Turn chronology (complete).** Implement TIM-1 with one atomic initial-user/turn fact and one typed terminal
   audit fact. *Closes when* queue/recovery preserve IDs/times without idle writes, replay clocks or
   head advances; partial-turn head targets and duplicate/missing/wrong-owner terminals mutate nothing.
2. **Context atoms and replay compatibility (active).** Project one selected head into typed message,
   reasoning/replay and complete tool-batch atoms. Add the compatibility value that decides whether
   opaque replay may enter a request. *Closes when* live and reloaded heads produce equal ordered
   atoms, head mutation cannot split an atom, parallel batches are indivisible,
   incompatible replay is typed, and both codecs preserve compatible ordering.
3. **Request attempts and immutable usage.** Implement TIM-2–TIM-5 with agent-step and compaction
   owners over an exact atom boundary/fingerprint; retire cumulative journal usage. *Closes when*
   pre-dispatch cancellation/encoding failure, dispatched terminal paths, missing usage and process
   death retain correlation without fabricated timing or delta writes; unmatched, duplicate,
   changed-owner/prefix/environment and invalid milestone/usage terminals mutate nothing.
4. **Budget ledger.** Give every atom and request-environment input a deterministic estimated cost;
   reconcile a provider usage report only with the exact prefix/fingerprint it measured. Return
   typed `Fits`, `CompactionNeeded` or `ImpossibleItem` decisions with separate response reserve and
   hard limit. *Closes when* suffix growth, changed tools/instructions, encrypted replay, maximal
   tool output and provider-total replacement have boundary tests without reporting missing usage
   as zero.
5. **Pure compaction plan.** Select a covered prefix and byte-exact retained suffix in atom units;
   build a summarization request as an append-only extension of the old request. Use a bounded
   verbatim → fitted → lossy input ladder and retain source identities plus current user/workspace
   context. *Closes when* repeated planning is deterministic, no tool batch can straddle a cut,
   every degradation is typed, the verbatim path retains the exact ordered provider-input prefix,
   and fitted/lossy paths carry their cache-break cause.
6. **Durable checkpoint.** Add one versioned checkpoint payload and projection rule. Commit its
   summary, provenance, source revision, compatibility and cache epoch before selecting the
   replacement view; resume derives checkpoint plus suffix from the same journal. *Closes when*
   deleting projections and reopening yields byte-identical requests, source history remains
   reachable, stale-head commit is refused, malformed structure or provenance is a typed journal
   projection failure, and runtime model/codec incompatibility refuses only context projection.
7. **Bounded automatic orchestration.** Invoke compaction before a turn at the soft threshold,
   after a tool result that would cross the hard bound, and once in response to a typed provider
   context error. Own and cancel the summarizer like any model task; cap attempts per turn and keep
   the old head usable on failure. *Closes when* each trigger has one deterministic journey, no
   retry loop or effect starts before journal acknowledgement, failures are visible, and the next
   request retains the checkpoint epoch's exact ordered provider-input prefix.
8. **Manageable heads and rewind journey.** Expose create, select, rename, abandon and rewind over
   the existing revision-checked head mutations, with current head and cache-break reason visible.
   Rewind to a stable atom boundary and create an alternate named head without copying history or
   re-executing work. Rewinding to a user item selects its preceding stable boundary and returns its
   text as a draft. *Closes when* branch operations preserve both exact requests, exit
   before resubmission cannot create recovery, and the agreed interaction is reviewed in three widths.

## Order, and why

Turn chronology fixes lifecycle boundaries. Atoms then define the exact identity consumed by
request timing and immutable usage. The ledger supplies measured pressure
without mutating history; the pure plan fixes compaction semantics before a durable payload freezes
them. Checkpoints land before automatic triggers, so recovery exists before background policy can
create them. The user-facing rewind journey comes last because it composes every prior boundary and
needs an interaction decision rather than a speculative command framework.

Rejected: destructive transcript rewrite, a second chat-history authority, index-based tool cuts,
provider response IDs as recovery state, and unbounded persistence or compaction queues.

## Deliberately not in this plan

Physical garbage collection, cross-session mail, durable permission grants, MCP, additional
provider transports, slash-command infrastructure, animation, themes and layout configuration.
