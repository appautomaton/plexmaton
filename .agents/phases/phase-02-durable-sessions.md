# Phase 02 — Durable sessions and context

| Field | Value |
| --- | --- |
| Status | Active; stage 1 and stage 2 slices 1–4 complete; stage 2 request attempts next |
| Parent roadmap | [Plexmaton Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Depends on | Phase 01's live loop, replay-authoritative model record, provider codecs, transcript reducer and native-tool lifecycle |
| Unlocks | Phase 03 — durable multi-agent mailbox and runtime ownership |

## Outcome

A session survives process exit as one inspectable JSONL journal. Its immutable entries form a
tree, named heads choose paths through it, and the same path deterministically rebuilds the
conversation and the provider request. Rewind never repeats an effect. Context compaction appends a
checkpoint on one branch and preserves the source entries, exact compatible provider replay, and a
stable cache epoch for later requests.

## Inherited

Stage 1 replaced the split in-memory histories with one typed journal. The agent and TUI now project
the selected path, while an owned JSONL writer preserves acknowledged facts. Explicit `create` and
`resume` journeys recover torn tails and unfinished turns without replaying effects (JRN-4–JRN-7).

`ProviderReplay` carries a typed route owner, codec identity/revision and model family, bounds its
private payload, redacts `Debug`, and round-trips losslessly through the session journal with
constructor validation (PRV-3). An explicit lossless export remains later work. Replay never enters
the visible transcript, ordinary diagnostics, or a redacted export.

User-owned configuration and runtime state remain under `~/.plexmaton/`; `PLEXMATON_HOME` redirects
the whole root for isolated development and tests (PRV-6). A project-local `.plexmaton/`, XDG
discovery, SQLite and redb are not introduced.

## Scope

1. **Canonical JSONL journal.** One file per session under `PLEXMATON_HOME/sessions`, with one
   date-epoch header carrying session identity and canonical creation time, followed by append-only
   records. Entries have stable identities and parent links;
   mutations carry one monotonic sequence. One owned writer serializes append and applies a record
   to memory only after the write succeeds. Loading keeps the longest valid prefix: a complete final
   JSON value missing its newline is repaired, an incomplete tail is isolated, and corruption before
   the tail is a typed failure. No database or derived on-disk index exists before measured need.
2. **Heads, rewind and resume.** Named heads are durable mutations over the immutable entry graph.
   Every append and head move checks the expected revision even within one process. Rewind selects a
   prior entry; replay rehydrates recorded outcomes and never executes a tool, asks for approval or
   contacts a provider. An unfinished final turn is visible as interrupted, and the user is told
   what recovery retained.
3. **Context projection and compaction.** A pure projector walks one selected head, keeps tool
   call/result batches paired, and produces the shared `ModelRequest` before a codec sees it.
   Request budgeting combines provider-reported usage for an identical prefix with an estimate of
   the unmeasured suffix, instructions, tool schemas and output reserve. Compaction generation
   extends the old request to preserve cache eligibility; its checkpoint shadows a covered range on
   that branch without deleting or copying authoritative entries. Every non-append projection change
   names its cache-break cause.
4. **Durable policy and remaining adapters.** Restored pending approvals rerun admission and current
   policy (APV-6); session- and workspace-scoped grants become explicit policy records. Remaining
   provider transports and MCP adapt to the same journal, context and admission boundaries rather
   than adding another loop or transcript.

Each stage receives a sliced plan when it starts. Stage 1 delivered the journal foundation and the
head mechanics in scope items 1–2. [Stage 2](../plans/phase-02-stage-02-context-projection.md)
finishes their production journey and owns scope item 3; later stages consume both.

## Not in this phase

A second live agent, delegation, cross-session mail delivery, pause/abort and the composed
status-and-artifact surface: Phase 03. Physical garbage collection of abandoned branches, a general
database/index layer, themes, animation and layout configuration: later measured work.

## Exit gate

| Criterion | Evidence expected |
| --- | --- |
| Exit and process death preserve every completed session fact | A real CLI session reopens from its JSONL file with both projections equal to the pre-exit state, after a clean exit and after the process is killed mid-session |
| Recovery is simple and visible | Complete missing-newline tails repair; incomplete final records are isolated; an unfinished turn is interrupted; the user sees one notice |
| Branches are durable and manageable | Create, move, rename and abandon named heads; reload preserves each selected path without copying entries |
| Replay has no effects | Rebuilding every head performs no network, tool, approval or filesystem effect |
| Context is deterministic and tool-safe | Every head produces byte-stable provider input; no projection or compaction splits a call/result batch |
| Compaction preserves history and cache intent | Original entries remain reachable; repeated compaction is stable; the summarization request extends the prior prefix and later requests extend one new epoch |
| Opaque replay survives safely | Lossless reload and explicit export/import preserve exact encrypted payload; Debug, transcript and default diagnostics contain none |
| Durable permission semantics close APV-6 | Restored pending work reruns current admission/policy and session/workspace grants have named revocation behavior |
| No second semantic spine appears | Agent and TUI projections reduce from the journal; provider, storage and MCP remain adapters; crate-graph gate passes |
