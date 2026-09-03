# Plan — Phase 01 stage 2, the producer

| Field | Value |
| --- | --- |
| Phase | [Phase 01 — One real agent](../phases/phase-01-one-real-agent.md) §scope 2 |
| Contract | [UI/UX](../ui-ux.md) §product vocabulary, §progressive disclosure, §state matrix |
| Status | Slices 1–9 of 10 landed 2026-09-03; slice 10 next |
| Blocked | None |

## Outcome

A real model streams into the primary conversation and runs tools there, behind a semantic
boundary that did not move. The projection cannot tell that the producer changed from a scripted
timeline to a model: it still receives `SessionEvent` on one monotonic sequence, still refuses a
gap, and the checked-in frames still match.

## The loop's contract

[`agent-loop.md`](../specs/agent-loop.md) owns LOOP-1 through LOOP-6 now that their identifiers are
cited by the implementation and tests. [`tool-admission.md`](../specs/tool-admission.md) owns the
admission and approval boundary. This plan owns only the order in which the mechanisms land.

## Slices

1. **The turn machine.** Landed. `plexmaton-agent`: the session record, the turn state, and a
   `handle` that takes a typed input and returns events plus effects. One turn is one step, because
   no tools exist yet to ask for a second. Closed by a stream the projection accepted with an empty
   notice log, and by a gate that refuses a runtime, a client or a terminal anywhere in this
   crate's dependency closure.
2. **Steps, tool calls, and the debt.** Landed. A turn is several steps under a budget counted in
   them (LOOP-1), a step dispatches its calls as one batch and is answered in model order
   (LOOP-3), and an interrupt or a failure pays what the batch owes before going idle. Closed by
   the debt asserted at every point an interrupt can land, shown to fail when the payment is
   removed.
3. **Input routing.** Landed. Submitted messages queue for the next turn, steering queues for the
   current turn's next step, and each bounded route is claimed only while its boundary opens
   (LOOP-6). An input with no boundary returns with its exact text and a typed reason. The
   composition root maps visible routes and `Ctrl-C` to `Input`; its synthetic adapter reports that
   it cannot perform an interrupt, and slice 6 replaces that adapter with the live loop owner.
4. **Tool admission and approval.** Landed. The model's untrusted name and raw arguments cross an explicit
   admission effect into an immutable call with typed capabilities, then a policy returns `Allow`,
   `RequireApproval`, or `Forbidden`. Each protected call waits in its own turn-owned slot; admitted
   siblings may run, while the completed batch remains model-ordered. The first decision vocabulary
   is only `AllowOnce` and `Deny`; pending has no approval timeout, and interrupt or shutdown pays
   its debt with a typed cancellation (APV-1 through APV-6). Closed by the badge and Attention
   count are projections of that state alone, a decision resumes or denies only the call it names,
   all cancellation and stale-decision paths are proven, and the user reviewed the checked-in
   wide, medium and narrow approval frames on 2026-09-02.
5. **The provider adapter.** Landed. `plexmaton-provider`: the selected local OpenAI-compatible endpoint,
   with explicit Responses and Chat Completions codecs, SSE to semantic model events, typed stop
   and error categories, bounded argument assembly, and exact plaintext/encrypted reasoning replay
   (PRV-1 through PRV-7). `~/.plexmaton/config.toml` is the normal root and `PLEXMATON_HOME`
   redirects the whole root for isolated development. *Closes when* sanitized recordings of both
   protocols drive a full tool round trip under `cargo test` with no network.
6. **Live text and reported usage.** Landed. A `plexmaton-runtime` owner drives one real `Agent` through the
   selected codec and a pooled HTTP client, with step-correlated output, owned cancellation and
   checked per-turn usage ([live-runtime](../specs/live-runtime.md) LIVE-1 through LIVE-6). No tools
   are advertised yet. *Closes when* sequential real conversations stream through the TUI,
   `Ctrl-C` cancels and joins one in flight, both protocols report usage through sanitized fixtures,
   deterministic failure tests leave no task alive, and the projection retains exact per-turn
   usage for a later on-demand diagnostics surface without adding permanent chrome.
7. **Read and search.** Landed. A workspace-rooted read capability, exact bounded UTF-8 windows with an
   authoritative observed version, and typed bounded search through an argv-based ripgrep adapter.
   *Closes when* a large file, a giant line, an ignored binary, a symlink escape and more matches
   than fit all end as bounded typed results without scanning or retaining unbounded work.
8. **File mutation.** Landed. The model trial selected exact observed replacements plus separate
   create-if-absent; [workspace-mutation](../specs/workspace-mutation.md) owns the canonical byte
   splices, descriptor-pinned publication, bounds and honest concurrency guarantee (MUT-1 through
   MUT-6). Stale, ambiguous, overlapping, concurrent, cancelled and failed operations preserve the
   original or absence, while LF, CRLF, BOM, tabs, ordinary permission bits and untouched Unicode
   bytes survive.
9. **Command.** Landed. A workspace-scoped foreground process with a typed exit cause, owned cancellation
   and two-phase termination, bounded UTF-8 stdout and stderr, and a broad declared capability.
   *Closes when* a megabyte on each stream drains to one bounded model result, and interrupt leaves
   no live member of the owned process group or unjoined drain. A deliberate new session/process
   group is outside this Unix boundary and is stated as such in
   [command-tool](../specs/command-tool.md), rather than hidden behind a stronger claim.
10. **Integrated live tools.** The live runtime registers the native catalog and executes admitted
   read, search, mutation and command effects; the simulator stays the deterministic test producer.
   *Closes when* a real model completes a representative coding task through tools, all failure and
   cancellation debts settle, a maximal command result cannot grow the transcript or next request
   beyond its bound, the notice log stays empty, and wide, medium and narrow frames have been looked
   at.

## Order, and why

1 first because every later slice is expressed in its states. 2 before 3, because what an
interrupt owes constrains what a queue may do at a boundary. 4 before 5, because approval is state
the adapter must never own, and writing it second invites the adapter to keep it. 5 before 6,
because the codec must be proven before a task owns its network stream. 6 comes before native tools
so real conversation, cancellation and usage receive user feedback early instead of waiting for the
whole coding surface. 7 before 8 because an edit's integrity precondition is the version the read
boundary observed. 9 is separate because process ownership and output drainage are not filesystem
mutation; 10 composes the already-proven pieces rather than discovering their contracts live.

Rejected: waiting until the final slice for any real request. The local proxy and credential path
now exist, so delaying the vertical path would make the user review four subsystems only after they
had all accumulated integration assumptions.

The first provider is the local OpenAI-compatible proxy at a configurable base URL, with
`gpt-5.6-luna` as the development model. Rejected: xAI first, because it was not the endpoint the
product's real development loop would exercise.

## Deliberately not in this plan

Persistence and replay, pre-request token estimation, context projection and compaction, a second provider, MCP, session- and
workspace-scoped grants, and the durable policy store behind them: Phase 02. A second agent,
delegation, mail, and the mailbox: Phase 03. The transcript grammar for tool calls, diffs and
reasoning, and retiring the Activity panel: stage 3 of this phase. Streaming a tool's partial output
while it runs, which stage 3 will want and this stage must not design blind.
