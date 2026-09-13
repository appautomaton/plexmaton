# Phase 02 — Durable sessions and context

| Field | Value |
| --- | --- |
| Status | Active; journal, providers, compaction, permissions and AGENTS.md complete; rewind locally verified, PR CI pending; MCP optional |
| Parent roadmap | [Plexmaton Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Depends on | Phase 01's live loop, replay-authoritative model record, provider codecs, transcript reducer and native-tool lifecycle |
| Unlocks | Phase 03 — durable multi-agent mailbox and runtime ownership |

## Outcome

A Conversation survives process exit as one inspectable JSONL journal. Its immutable entries form a
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
the whole root for isolated development and tests (PRV-6). SKL-1 separately owns project model
selection; XDG discovery, SQLite and redb are not introduced.

## Scope

1. **Canonical JSONL journal.** One file per session under `PLEXMATON_HOME/sessions`, with one
   date-epoch header carrying session identity and canonical creation time, followed by append-only
   records. Entries have stable identities and parent links;
   mutations carry one monotonic sequence. One owned writer serializes append and applies a record
   to memory only after the write succeeds. Loading keeps the longest valid prefix: a complete final
   JSON value missing its newline is repaired, an incomplete tail is isolated, and corruption before
   the tail is a typed failure. No database or derived on-disk index exists before measured need.
2. **Heads, rewind and resume.** Named heads are durable mutations over the immutable entry graph.
   Every append and head move checks the expected revision even within one process. Selection and
   rewind follow [context epochs and branch selection](../ui-ux.md#context-epochs-and-branch-selection).
   Replay rehydrates recorded outcomes and never executes a tool, asks for approval or
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
   policy (APV-6); coding-session grants are runtime state retained across conversations, while
   workspace grants have durable records. Remaining transports use the same boundaries rather
   than adding another loop or transcript. MCP is optional later integration, not an exit gate.
5. **Project instructions.** [Agent instructions](../specs/agent-instructions.md) load applicable
   `AGENTS.md` files into the current request environment, with explicit scope and bounded reads.

Each stage receives a sliced plan when it starts. Stage 1 delivered the journal foundation and the
head mechanics in scope items 1–2. [Stage 2](../plans/phase-02-stage-02-context-projection.md)
finishes their production journey and owns scope item 3, constrained by the
[compaction spike](../spikes/compaction/README.md). Stage 3 delivered the provider transports
in scope item 4. Stage 4 delivered [permission policy](../specs/permission-policy.md)
and the reviewed approval flow. Current-main integration and public readiness are verified;
their consumed plans are removed. Stage 5 delivered [AGENTS.md support](../specs/agent-instructions.md).
Branch interaction remains separate work; MCP is optional.

### AGENTS.md instructions — complete

AGI-1–AGI-5 define bounded, attributed instruction discovery and one immutable conversation
environment across all four dialects. The [source comparison](../spikes/agent-instructions/README.md)
records the local Codex, Grok, Pi and Kimi evidence. The consumed stage plan is removed.

On 2026-09-07, the feature worktree based on `1393bcb` passed 244 provider/CLI tests, including
15 new instruction regressions, and all-target Clippy. A three-request loopback journey proves
stable instructions during an open Conversation, fresh instructions on JSONL resume, and no
instruction text or replay-only mutations added to the journal. Startup failure is tested through
the real executable before terminal acquisition or session creation. Formatting, citation, crate
graph, file-length and typo checks passed; no dependency or rendered frame changed. Document
budget warnings remain for the UI contract and root README. No live model, PTY smoke, performance
benchmark or CI run was part of this stage. Nested instruction discovery is model-directed;
actual model adherence remains unverified.

### Provider dialects — complete

[PRV-1–PRV-7](../specs/provider-adapter.md) define Responses, Chat Completions, Messages and native
Gemini GenerateContent over the same canonical journal and loop. The
[spike](../spikes/provider-adapter-parity/README.md) retains the source comparison. On base `3983bca`,
795 tests passed in the default parallel workspace suite, including JRN-4's inherited-descriptor
lock regression. Formatting, all-target compilation, Clippy, corpus gates, typos, machete and the
offline dependency audit passed; the audit retains existing hashbrown/syn duplicate warnings.

Real codec output survives JSONL reopen with exact replay and stable cache identity. Interrupted
reasoning and failures remain in the journal while subsequent live and reopened wire requests
match. Three-width frames were inspected for
[provider errors](../spikes/provider-adapter-parity/frames/provider-failure-wide.txt)
([medium](../spikes/provider-adapter-parity/frames/provider-failure-medium.txt),
[narrow](../spikes/provider-adapter-parity/frames/provider-failure-narrow.txt)) and
[interrupted thinking followed by continuation](../spikes/provider-adapter-parity/frames/interrupted-thinking-wide.txt)
([medium](../spikes/provider-adapter-parity/frames/interrupted-thinking-medium.txt),
[narrow](../spikes/provider-adapter-parity/frames/interrupted-thinking-narrow.txt)), using the
[real agent and TUI renderer](../spikes/provider-adapter-parity/render-review.rs).
No live provider/proxy inference, PTY smoke or performance measurement was run for this stage;
realized cache hits and provider billing remain unverified.

### Permission policy — complete

[PER-1–PER-10](../specs/permission-policy.md) and
[PGR-1–PGR-5](../specs/project-permissions.md) define the shared Session authority, two-step approval,
personal project grants, configured rules/trust and bounded literal command prefixes. The
[spike](../spikes/permission-policy/README.md) retains source comparisons and finite experiments.
The consumed stage and integration plans are removed.

On 2026-09-06, the final offline workspace run passed 964 tests across 47 suites, with no failures
or ignored tests. Formatting, all-target check/Clippy, corpus gates, typos, machete and the offline
cached-advisory audit passed; the existing hashbrown/syn duplicate warnings remain. Twenty Python
boundary tests and all three terminal smokes passed using isolated state and loopback fixtures.

The [permission executable journey](../../scripts/smoke-permissions.py) activates reviewed project
rules before the first Conversation, verifies the actual command effect, saves an `ls` Project
prefix, restarts, reuses it for a different argument, revokes it and observes a denied tool result.
It makes exactly eight local fixture requests. Its captured terminal frames were inspected at
120, 95 and 60 columns; only the random temporary-directory suffix is normalized below.

| Actual executable surface | Wide | Medium | Narrow |
| --- | --- | --- | --- |
| Project rule review | [frame](../spikes/permission-policy/frames/cli-trust-wide.txt) | [frame](../spikes/permission-policy/frames/cli-trust-medium.txt) | [frame](../spikes/permission-policy/frames/cli-trust-narrow.txt) |
| Remember prefix | [frame](../spikes/permission-policy/frames/cli-prefix-wide.txt) | [frame](../spikes/permission-policy/frames/cli-prefix-medium.txt) | [frame](../spikes/permission-policy/frames/cli-prefix-narrow.txt) |

Other reviewed Ratatui frames retain their owning component evidence:

| Surface | Wide | Medium | Narrow |
| --- | --- | --- | --- |
| Native approval | [frame](../../crates/plexmaton-tui/frames/native-approval-wide.txt) | [frame](../../crates/plexmaton-tui/frames/native-approval-medium.txt) | [frame](../../crates/plexmaton-tui/frames/native-approval-narrow.txt) |
| Exact fallback | [frame](../../crates/plexmaton-tui/frames/remember-permission-wide.txt) | [frame](../../crates/plexmaton-tui/frames/remember-permission-medium.txt) | [frame](../../crates/plexmaton-tui/frames/remember-permission-narrow.txt) |
| Permission controls | [frame](../../crates/plexmaton-tui/frames/permission-controls-wide.txt) | [frame](../../crates/plexmaton-tui/frames/permission-controls-medium.txt) | [frame](../../crates/plexmaton-tui/frames/permission-controls-narrow.txt) |
| Drawer | [frame](../../crates/plexmaton-tui/frames/drawer-wide.txt) | [frame](../../crates/plexmaton-tui/frames/drawer-medium.txt) | [frame](../../crates/plexmaton-tui/frames/drawer-narrow.txt) |
| Trust confirmation | [frame](../../crates/plexmaton-tui/frames/project-trust-wide.txt) | [frame](../../crates/plexmaton-tui/frames/project-trust-medium.txt) | [frame](../../crates/plexmaton-tui/frames/project-trust-narrow.txt) |
| Saved grant after audit failure | [frame](../../crates/plexmaton-tui/frames/project-permission-receipt-wide.txt) | [frame](../../crates/plexmaton-tui/frames/project-permission-receipt-medium.txt) | [frame](../../crates/plexmaton-tui/frames/project-permission-receipt-narrow.txt) |

Current-main integration on 2026-09-06 passed 1,075 Rust tests, 22 Python tests, all-target
Clippy, formatting, crate/citation/file-length gates, typos and machete. The offline dependency
audit passed using an isolated copy of cached advisories; existing duplicate-crate warnings remain.
CPL-3/SKL-5 now retain the complete current skill invocation, and CPL-7/SKL-6 let skill completion
progress during a waiting summary. Their regressions are linked in the owning specs.

All three terminal smokes passed. Their settled permission frames above were inspected again;
the harness drains final terminal output before joining. The combined executable also passed
the direct-Kitty check at 120/88/60 columns, including exact formula copy and clean exit with
one local fixture request. All 15 regenerated [math frames](../../crates/plexmaton-tui/frames/math/reply-88.svg)
matched the reviewed assets byte-for-byte; wide/medium/narrow reply renderings were inspected.

Publication hygiene adds tested private-state/build ignores without hiding fixtures, skills,
project settings or Cargo.lock, and read-only CI authority with the script/corpus/terminal gates.
The repository is public, with anonymous access and the product-led README verified on 2026-09-06.
At `76bea5b`, [macOS Apple Silicon CI](https://github.com/appautomaton/plexmaton/actions/runs/34039859636)
passed every workspace, supply-chain and terminal gate with current stable actions and no annotations.
Process fixtures avoid the system Python launcher; production deadlines and lifecycle assertions are unchanged.
MUT-4 carries native Unix mode/device types through file publication.
History, staged source and all ten existing Actions logs passed local secret scans. Licensing is unchanged. The UI contract
retains a 748-byte soft-budget warning; no product rule was removed to silence it.
No live model, live configuration, command containment, multi-agent runtime or performance benchmark
was part of this integration. JRN-3 keeps `2026-09-04` and `2026-09-05` readable without header
rewrites. The rebuilt macOS CLI resumed an unchanged copy of an existing conversation with no
model request or journal write, then exited cleanly.

### Compaction — complete

[CPL-1–CPL-8](../specs/compaction.md) own the planner, checkpoint recovery and bounded runtime.
On 2026-09-06, the worktree based on `2bb0a70` passed 881 workspace tests, all-target check/Clippy,
formatting, corpus gates, typos, machete and 17 script tests. The offline supply-chain audit used
a local copy of the cached advisory database and retained existing duplicate-crate warnings;
no dependency changed. Both terminal and three-width status-line PTY smokes passed without a
model request.

The real agent/TUI renderer produced inspected failure frames at three widths:
[soft failure](../spikes/compaction/frames/soft-compaction-failure-wide.txt)
([medium](../spikes/compaction/frames/soft-compaction-failure-medium.txt),
[narrow](../spikes/compaction/frames/soft-compaction-failure-narrow.txt)) and
[hard failure](../spikes/compaction/frames/hard-compaction-failure-wide.txt)
([medium](../spikes/compaction/frames/hard-compaction-failure-medium.txt),
[narrow](../spikes/compaction/frames/hard-compaction-failure-narrow.txt)).
[Source](../spikes/compaction/render-review.rs) stages the same semantic outcomes without HTTP;
runtime tests separately prove their automatic triggers and commit barriers.
Validation is offline by policy; paid provider calls are not an exit gate. No claim is made about
realized cache hits or generated summary quality. Slice 10 has proven durable head selection,
destination navigation, acknowledged runtime results, bounded tree snapshots, labels and exact
source copy. Native interaction and executable acceptance pass locally; PR CI remains pending in the
[stage plan](../plans/phase-02-stage-02-context-projection.md).

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
| Rewind preserves both continuations | Each rewind forks a new head; before/between checkpoints it selects the target ancestry's base and leaves the original branch unchanged |
| Replay has no effects | Rebuilding every head performs no network, tool, approval or filesystem effect |
| Context is deterministic and tool-safe | Every head produces byte-stable provider input; no projection or compaction splits a call/result batch |
| Compaction preserves history and cache intent | Original entries remain reachable; repeated compaction is stable; the summarization request extends the prior prefix and later requests extend one new epoch |
| Opaque replay survives safely | Lossless reload and explicit export/import preserve exact encrypted payload; Debug, transcript and default diagnostics contain none |
| Permission lifetimes close APV-6 | Restored work reruns current admission/policy; conversation replacement retains coding-session grants, exit clears them, and workspace grants have durable revocation |
| Project instructions are scoped and accounted for | AGI-1–AGI-5 discovery, four-dialect encoding, budget/compaction and conversation-open tests |
| No second semantic spine appears | Conversation projections reduce from the journal; runtime permission state does not rebuild authority from history; crate-graph gate passes |
