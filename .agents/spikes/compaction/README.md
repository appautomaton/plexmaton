# Compaction spike

| Field | Value |
| --- | --- |
| Status | Source comparison and offline probes complete; production evidence lives in CPL-1–CPL-8 |
| Read when | Implementing checkpoint projection or compaction cache identity |
| Question | Can compaction preserve request-prefix eligibility, original history, and consistent context after reopening? |
| Contract | [Compaction](../../specs/compaction.md); BUD-2, JRN-1/JRN-3/JRN-5/JRN-7, PRV-3/PRV-4, TIM-3/TIM-4 |
| Decision | [Compaction](../../specs/compaction.md) implements [the context-epoch contract](../../ui-ux.md#context-epochs-and-branch-selection) |

## Corpus

Inspected on 2026-09-05 at Plexmaton base `2bb0a70f56660d95d4feb8f11ec7ead3336db5a7`.
Reference roots are relative to the original checkout; source citations are relative to each
reference. These local references may be absent in another clone.

| Reference root | Revision | State |
| --- | --- | --- |
| `../pi-arcweld/pi-mono` | `853a80d26c90a14c1886f0ebb8ffaae133ca2185` | Clean |
| `.references/pi_agent_rust` | `b7b5988b3a4ee83cb2baae24c6a44fb182c68e58` | Clean |
| `../codex` | `316795b3cf2a45e90d121d9f46499d4658b2645c` | Clean |
| `../grok-build` | `72a61251fcffb464bcc687aeb5a998e5a98ec0c9` | Clean |

Reference implementation and test source were inspected; no reference tests executed. The targeted
Grok command `cargo test -p xai-grok-compaction select::tests::snaps_past_tool_results -- --exact`
stopped before compilation because rustup could not write its temporary directory. No network,
live provider call, reference edit, or toolchain installation followed.

## Comparison

**Pi TypeScript.** The shipped coding agent uses a standalone summarizer system and one serialized
conversation message, omits tool definitions, and requests `cacheRetention: "none"` with fresh
routing when no session ID is supplied. Its normal compaction caller supplies none. This does not
preserve the prior request prefix. Sources: `packages/coding-agent/src/core/compaction/compaction.ts:579`
and `:656`; `packages/coding-agent/src/core/agent-session.ts:1898`. The characterization test at
`packages/coding-agent/test/suite/agent-session-compaction.test.ts:259` explicitly checks that shape.

Its `firstKeptEntryId` checkpoint retains original entries and projects the latest summary plus
retained suffix (`packages/coding-agent/src/core/session-manager.ts:410`). Missing retained
provenance silently loses that suffix. Cuts exclude tool-result roles but do not validate complete
parallel call/result identities (`packages/coding-agent/src/core/compaction/compaction.ts:308`).
The newer helper's `AgentHarness.compact()` is unimplemented
(`packages/agent/src/harness/agent-harness.ts:350`); it is not shipped lifecycle evidence.

Pi exposes `compaction.keepRecentTokens` (default `20000`), `reserveTokens` (default `16384`)
and `enabled` in `settings.json` (`packages/coding-agent/docs/settings.md:117`). Its backward scan
targets that recent-token count and moves the cut to a valid entry; a mid-turn cut receives a
separate turn-prefix summary (`packages/coding-agent/src/core/compaction/compaction.ts:388`, `:764`).
CPL-3 adopts a configurable recent-tail target over Plexmaton's indivisible atoms; CPL-2 keeps
the complete normal request prefix instead of Pi's standalone summarizer request.

**Pi Rust.** Its standalone summary request also changes system/context/tools (`src/compaction.rs:1593`).
The useful publication pattern is a private candidate, save/reconcile, then install; uncertain
persistence blocks provider admission (`src/agent.rs:12883`, failure tests at `:19060`). Adopt the
ordering through JRN-7, without copying a whole session. Its worker carries session/model/leaf
origin, attempt limits, timeout and cancellation (`src/compaction_worker.rs:168`, `:217`, `:485`).
Its emergency deterministic summary is degradation evidence, not a cache-preserving request.

**Codex.** Local compaction appends an instruction but constructs a default `Prompt` without the
normal tools (`codex-rs/core/src/compact.rs:278`). Native compaction carries tools and can retain an
opaque provider compaction item (`codex-rs/core/src/compact_remote_request.rs:23`);
that protocol-specific mechanism is not available across Plexmaton's four dialects. Recovery
replays a persisted replacement and later suffix; a cold-resume prefix test exists at
`codex-rs/core/tests/suite/compact.rs:5423`. Live replacement precedes rollout persistence
(`codex-rs/core/src/session/mod.rs:3650`), so its publication ordering does not satisfy JRN-7.

**Grok Build.** This is the closest request-construction reference: append one summary instruction
and keep the effective tool definitions. Sources under `crates/codegen/xai-grok-shell/src/`:
`session/helpers/prepared_compaction_history.rs:48`, `session/helpers/session_compact.rs:434`;
prefix test at `session/helpers/prepared_compaction_history_tests.rs:81`. Its typed
verbatim → fitted → lossy ladder advances on context overflow (`session/compaction.rs:1071`, `:1137`).
Image preparation and backend reasoning filtering make verbatim behavior conditional; these
sources do not prove Plexmaton's request bytes or realized cache hits.

Grok enqueues a separate checkpoint blob and update marker before replacing live conversation
(`session/compaction.rs:1770`, `:2290`). Neither send awaits storage acknowledgement; worker failures
are warnings. Queue order is not JRN-7 acknowledgement, and the two-file authority does not fit
Plexmaton's journal. Its role-based fitting remains weaker than JRN-5 atoms.

## Executed probes

Run from this worktree's root:

```sh
./.agents/spikes/compaction/run-codec-spike.sh
```

The runner builds existing crates offline in this worktree's `target/compaction-spike`, refuses
missing or ambiguous rlibs, and runs both probes without reference checkouts or configured providers.

| Probe | Result | Evidence |
| --- | --- | --- |
| [Journal projection and four encoders](./codec-prefix.rs) | 5 tests passed | Exact append with tools/replay; changed instructions/tools and flattened history as counterexamples |
| [Finite checkpoint/budget model](./budget-boundary-model.rs) | 4 tests passed | Existing span lookup can select stale measurements after a checkpoint; epoch and ordered-prefix oracle refuses them |

[Codec method and limits](./codec-evidence.md) own the wire-prefix claim and fixtures. No whole
HTTP-body byte-prefix assertion or provider cache-hit measurement is made.

The budget model reproduces the baseline `ConversationJournal::budget_basis` lookup, which ordered
atoms by original journal-position spans. A checkpoint appended after a retained suffix
but projected before it breaks that assumption. An old attempt can appear to measure the entire
replacement, or zero atoms while still carrying the old conversation's input count. The finite
model exercises this algorithm rather than the production checkpoint API. Removing its epoch check
makes the epoch/prefix witness fail (mutation checked); CPL-5 owns the implemented correction.

## Production evidence

[CPL-1–CPL-8](../../specs/compaction.md) own the implementation and named regression tests.

The real agent/TUI renderer produced inspected failure frames at three widths:
[soft failure](./frames/soft-compaction-failure-wide.txt)
([medium](./frames/soft-compaction-failure-medium.txt),
[narrow](./frames/soft-compaction-failure-narrow.txt)) and
[hard failure](./frames/hard-compaction-failure-wide.txt)
([medium](./frames/hard-compaction-failure-medium.txt),
[narrow](./frames/hard-compaction-failure-narrow.txt)).
[Source](./render-review.rs) stages the same semantic outcomes without HTTP;
runtime tests separately prove their automatic triggers and commit barriers.
Summary quality, exact token sufficiency and realized provider cache hits remain unverified.
