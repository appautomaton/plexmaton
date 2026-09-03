# Plan — Phase 01 stage 3, transcript grammar

| Field | Value |
| --- | --- |
| Phase | [Phase 01 — One real agent](../phases/phase-01-one-real-agent.md) §scope 3 |
| Contract | [UI/UX](../ui-ux.md) §progressive disclosure, §readability, §transcript grammar, §state matrix |
| Status | Slices 1–6 done; slice 7 ready |
| Blocked | None |

## Outcome

One conversation tells its agent story. Messages, reasoning, tool calls, applied diffs,
artifacts, mail, system notices, warnings and errors occupy one stable ordered transcript. A tool
is one compact entry throughout its lifecycle and opens to bounded semantic detail. Activity is
gone; copy returns source, and current work appears statically at the composer.

Reducing the same ordered facts reconstructs the transcript without invoking provider, policy or
tool. This stage does not persist or branch history, but Phase 02 will not replace its identities or
maintain a second live/replay UI.

## Constraints established before implementation

- The producer assigns and serializes a first-party entry identity which fixes first appearance.
  Domain IDs remain correlations, never substitutes; vector indexes are not identities.
- A tool result updates its call's original entry. Execution completion may be unordered, model
  results remain in model call order, and visible entries remain in first-appearance order.
- Live traffic and future restore use one pure semantic reducer. Replaying a call/result pair never
  executes the effect, asks for approval, or contacts the model. A crash-restored pending approval
  remains subject to APV-6 rather than becoming an executable historical command.
- Compact, open and copied forms project one bounded source. Fold, selection, hover and render
  caches are view state, never `SessionEvent` or model context.
- The tool boundary produces typed presentation from admitted input and actual outcome. The TUI
  does not match tool names, parse model JSON, inspect files, or reconstruct a diff.
- Encrypted reasoning remains provider replay only. Only explicit plaintext reasoning or summary
  can become a visible entry.

## Slices

1. **Replayable entry spine — done.** Landed the serial foundation: the producer-assigned identity, owner,
   order, revision and typed presentation envelope for every Stage 3 category. Text, tool,
   artifact, mail, system, warning and error facts all enter one typed model; contract violations
   remain in the notice log. A call creates one position and every later state, including `Denied`,
   updates it. Model, completion and display order stay explicit. *Closes when* shuffled sibling
   completions update their original entries, invalid transitions/revisions are refused, events
   round-trip through JSON, and two fresh projections of the same envelopes are equal.

2. **Native presentation facts — done.** Populated Slice 1's envelope for read, search, create,
   edit and command. Invocation comes from admitted data and completion from the actual result.
   Text carries explicit omission metadata; edit retains a complete canonical patch under a hard
   bound derived from MUT-6, never a later reconstruction or partial patch. Success, failure,
   denial, cancellation and stale mutation have bounded facts, and the maximum valid edit retains
   its complete copy source.

3. **One compact transcript — done.** Project messages, tools, artifacts and mail in unified order; paint
   queued, running, approval required, succeeded, failed, denied and cancelled tools as one compact
   line. Retire Activity but keep its counts on each agent row. Completion changes one entry, hence
   one re-wrap. *Closes when* interleaved text/tools survive out-of-order completion, medium and
   narrow lose nothing, TR-1/TR-2 hold, and all seven state frames are reviewed.

4. **Current work at the composer — done.** Derive one static status from semantic state. Accepted copy is
   `Thinking`, `Responding`, `Running <tool>` and `Approval required`; action required outranks
   background work and idle adds no label. Put it in the composer's top boundary so transcript and
   input do not jump. *Closes when* priority is table-tested, repeated facts cost no frame, no timer
   exists, and the user has reviewed wide/medium/narrow frames before accepted copy enters UI/UX.

5. **Disclosure, pointer feedback and copy — done.** A selected entry opens in place and participates in
   its parent transcript's scrolling; no nested surface or viewport exists. Pointer move may
   highlight a compact foldable row but changes no focus or semantic state, and lands only with a
   user-reviewed frame. Copy returns retained source without gutters, clipping or decoration.
   *Closes when* resize/scroll/theme do not change copy, opening invalidates only that entry's
   height, keyboard and pointer agree, and monochrome remains sufficient.

6. **Diff and remaining grammar — done.** Paint the complete retained patch with `+`/`-` markers and
   semantic roles; viewport clipping and decoration limits never alter its copy source. Give
   reasoning, system, warning and error their treatments; encrypted replay stays absent. *Closes
   when* diff meaning survives monochrome, expensive decoration degrades to bounded plain text,
   copy returns the complete patch, and every role has a reviewed state frame.

7. **Real traffic and phase gate.** Drive a real read → edit → command turn through the unified
   transcript, including approval, failure and cancellation. Add frame workloads for compact/open
   entries and confirm the notice log stays empty; update all specs and checked-in frames, run every
   gate, and obtain user review at wide, medium and narrow. *Closes when* every Phase 01 exit
   criterion is evidenced, this consumed plan is deleted, and Phase 01 can be assessed for closure.

## Worktree delivery map

Slice 1 ran alone in `.worktrees/stage3-entry-spine`; every later slice imports its event and
presentation shape. Wave B branched from its reviewed commit:

| Worktree | Slice | Exclusive ownership during the wave |
| --- | --- | --- |
| `.worktrees/stage3-presenters` | 2 | Agent/runtime result seam plus file/command presenters; no TUI or shared type edits |
| `.worktrees/stage3-transcript` | 3 | TUI entry projection, transcript measurement/content and Activity removal; no executor code |
| `.worktrees/stage3-status` | 4 | Current-work derivation and composer chrome only; no transcript-entry or tool-result types |

Each used its own `target/`, landed conventional commits, passed its gates and received independent
read-only review. Root merged 2–4 in order and ran the workspace gates before removing the trees.

Slices 5–6 run sequentially from merged Wave B because both change measurement, selection and
rendering. Slice 7 runs only on integrated main.

## Order, and why

Identity and replay semantics come before presentation because Phase 02 must be able to select a
branch path and feed it to this reducer without changing what an entry is. Native facts and TUI
projection then share only that contract and can proceed in parallel. Current-work chrome is a
separate projection and can join the same wave. Disclosure precedes diff and role polish because
their full forms need one established open/copy path. Real traffic comes last to validate
composition, not to discover the contracts.

Rejected: a UI-only tool history beside the model record, because restore would need reconciliation;
re-executing effects during replay; using completion order or vector position as transcript order;
one universal string detail parsed by widgets; and opening every worktree before the shared entry
contract lands.

## Deliberately not in this plan

Storage, JSONL/SQLite, parent links, rewind, fork, compaction, cache-preserving request prefixes and
`/resume`: Phase 02. A second live agent, delegation records and durable mail: Phase 03. Theme
loading, slash commands, configurable layout and a general animation clock: later work. This plan
uses the accepted pastel palette and adds no spinner.
