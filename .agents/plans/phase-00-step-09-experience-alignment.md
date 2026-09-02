# Plan — Phase 00 step 09: align the experience with the contract

| Field | Value |
| --- | --- |
| Phase | Phase 00, reopened on 2026-09-01 for one step before Phase 01 begins |
| Contract | [`ui-ux.md`](../roadmap/ui-ux.md) §experience promise, §progressive disclosure, §readability, §responsive layout classes, §transcript grammar; mechanism changes land in [inspector](../specs/inspector.md) and [transcript-layout](../specs/transcript-layout.md) |
| Status | slice 1 of 5 landed; slice 2 next |

## Outcome

The screen the prototype paints is the one `ui-ux.md` describes: tool calls, mail and artifacts
live in the conversation and open on demand; the conversation dominates at every width; the second
window shows a different agent than the first, in words a user recognizes; and hierarchy comes from
spacing and colour before boxes. The mechanisms Phase 00 proved — surfaces, viewports,
virtualization, routing, copy — are unchanged underneath.

Why this exists: the exit gate on 2026-08-31 was assessed by text-presence tests, and the
"wide, medium and narrow screen compositions" it promised were never produced. The user compared
the running prototype against the contract on 2026-09-01 and found the mechanisms built and the
experience not.

## Slices

1. **The composition the contract describes.** Landed 2026-09-02; the compressed record is the
   phase file's step 09 entry. The second window is the selection and floats (D-049); one agent
   column with the list over the activity (D-014); every input inside its conversation's box, the
   composer collapsing to one row and `Tab` returning to it; strips at the top; equal columns at
   ultrawide; titles in the heading role. Verified in a pseudo-terminal, not only headless.
2. **Vocabulary and a scenario that exercises the grammar.** `plexmaton-core` gains what the
   transcript grammar needs and the events cannot yet say: a `Reasoning` transcript role, a typed
   `detail` on tool activity (text or diff, bounded), and an `AwaitingApproval` tool state. All
   additive: serde defaults, no renamed tag. `Scenario::canonical` grows so every grammar row
   appears once — a long streamed message, reasoning, tools in every state including a failure and
   an approval, a diff, mail, an artifact, a runtime warning, and a third agent that fails — without
   moving the ticks the journey tests stand on. Proves: core round-trip tests for each addition;
   the existing journey and smoke still pass; a headless render at 80, 100 and 140 columns is
   attached to the step record as the baseline. Unblocks: 3.
3. **Transcript grammar.** Tool activity, mail and artifacts become entries in the owning agent's
   conversation in arrival order, replacing the Activity column as their home. A tool entry is one
   compact row with a state marker and opens under the selection to show its detail; diff detail
   paints added and removed lines distinctly; reasoning, system, warning and error rows each have
   one treatment, readable without colour. Entry heights are keyed by revision, width and
   open-state (TR-1 extended). Proves: a differential test that the virtualized conversation
   paints what the whole one does with mixed entries; open/close under both focus modes; copy
   returns a tool's detail and a mail's summary. Unblocks: 4.
4. **Composition.** The agent list is one row per agent with status on the
   same row, a column only at wide and above and a band elsewhere; the Activity column is retired
   and its counts move to the agent row; focus is shown by the border, not by title colour; the
   hint strip moves into the composer's bottom border; no user-facing word is `inspector`, `shelf`
   or `column`. The activity column's placement and the sub-agent-only list landed in slice 1.
   Proves: layout tiling tests at every class; the journey rewritten to the new grammar; the
   screenshot baseline from slice 2 re-rendered and compared by eye. Unblocks: 5.
5. **Record.** `ui-ux.md` layout-class table and vocabulary updated; D-042 and D-046 superseded;
   inspector and transcript-layout specs corrected; a compressed step entry in the phase file;
   this plan deleted.

## Order and why

The window came first because it was the one thing the user could not read past: the default path
showed one conversation twice, and every other judgement waited on that. The grammar needs content to be judged against, so the scenario comes first, and the vocabulary
comes with it because the events cannot carry a tool's output today. The composition depends on
the grammar: the Activity column can only be retired once its contents have a home in the
conversation. The record comes last because every earlier slice changes what it must say.

Each slice ends with real frames at three widths shown to the user, who rejected sketches in favour
of the rendered prototype. Nothing in a later slice is decided before the earlier frames are seen.

## Deliberately not in this plan

- Pause and abort controls. They need a runtime that can be told to stop; Phase 03 owns them.
- The composed five-domain inspector (D-046). Slice 2 moves tools, mail and artifacts into the
  conversation, which is where the contract says they are read; a separate metadata surface
  remains Phase 03's.
- Math rendering. The track owns it.
- Themes and animation. Phase 04.
