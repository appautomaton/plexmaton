# Spec — The second window

| Field | Value |
| --- | --- |
| Status | Implemented, wired and accepted; Narrow structure and function were approved in native Kitty on 2026-09-16 |
| Owns | What the second window shows, where it goes, and what opening, entering, resizing, and closing it do |
| Depends on | [surface-model](./surface-model.md) SURF-3–SURF-5; the Escape ladder in [interaction-routing](./interaction-routing.md) INV-6; the shelf and Narrow rules in [`ui-ux.md`](../ui-ux.md) |
| Proven by | `plexmaton-tui::layout::inspector`, `::state::inspector`, and `::workspace` tests |

The code calls this surface `Inspector` (ui-ux §product vocabulary). On screen it is the second
window: the user is talking to one agent and looking at another.

## Invariants

**INS-1 — The window is the committed selection.** The list holds only sub-agents and is registered
only when one exists. From Medium upward, arrow or click selection opens or repoints the second
window. Narrow replaces the current conversation with a full-region navigator whose temporary
cursor changes no window; `Enter` or a complete row click commits it and replaces Agents with that
child. `Escape` or `Ctrl-B` cancels navigation and restores the conversation underneath. Outside
that temporary cursor, nothing stores which agent is open apart from the selection, so no
conversation is on screen twice. Rejected: storing a pinned/followed window beside selection, which
put one conversation on screen twice.

**INS-2 — Ten readable rows stay beneath the window, or the window takes the region outright.**
There is no third outcome where a shelf and a squeezed conversation share a region too small for
both. A terminal with fewer than ten rows before anything opened may have none covered.

**INS-3 — Presentation is derived from size, never stored.** Shelf, column, or maximized is chosen
per frame from the layout class and the user's maximize; changing it changes no identity, scroll
position, or focus.

**INS-4 — Entering is explicit; closing gives focus back.** Looking at an agent leaves the keyboard
in the list, so arrows keep moving through it and `Enter` moves into the window. A Narrow navigator
visit records the exact focus that opened it and restores that focus on `Escape`, `Ctrl-B`, or a
resize into Medium. Rejected: focusing on look, which stops the arrows.

**INS-5 — The window's input requires CCV-2 control eligibility and window focus** (ui-ux §input). It takes a
strip off the bottom of the window's own rectangle, never off the conversation's guarantee, and
while it is active the primary composer collapses to one row that stays clickable and stays a focus
stop. A wheel over the strip addresses that window's conversation viewport. A rectangle with no room
for both keeps the conversation and shows no input.

**INS-6 — What the window shows is a conversation.** The looked-at agent's, through the same cache
and reading position the main conversation uses (TR-1, TR-3, TR-5), keyed by agent. Text, tools,
mail and artifacts share its first-appearance order and one viewport; there is no parallel detail
surface or regrouped order. Rejected: composing status and an artifact index into the window now,
which needs sub-region scroll ownership and an expand model the transcript lacks.

**INS-7 — A window with no room for its input is a navigation surface.** No input, no cursor, no
text target, no draft. One geometry function, called by the renderer and by focus, answers all
three, so the affordance, the caret and the keystroke cannot disagree.

**INS-8 — Only a shelf is resizable.** A maximized window or tiled column has no adjustable shelf
edge, so keyboard and pointer resize gestures are no-ops there. Returning to a shelf restores the
height the user chose rather than deriving one from a non-shelf rectangle.

## Model

```text
Roster                              layout::inspector
  selected ≠ primary  ──▶ open ──▶  presentation(class, maximized, rows)
  selected = primary  ──▶ closed         │
                                         ├─ Shelf      docked to the top of the conversation
state::inspector                         ├─ Column     the secondary column, at ultrawide
  { maximized, rows }  presentation      └─ Maximized  the whole conversation region
```

| Fact | Value |
| --- | --- |
| Shelf height | `min(⌊0.55 × region⌋, region − 10)` rows from the conversation's top, or the dragged height, clamped the same way |
| Fallback | Below eighteen rows of conversation, with the normal primary composer reserved, the presentation is maximized: a shelf of two borders and six lines is not worth being one. An eligible child input returns the primary composer's spare rows without turning that fallback into a shelf |
| Layering | The one surface above the base layer, drawn inside the conversation's rectangle so the conversation keeps that rectangle and its reading position. The cells beneath are cleared first |
| The column | At ultrawide, out of the conversation's width, never the agent column's (ui-ux §layout classes) |
| Maximize and dragged height | In `state::inspector`: they belong to the window, not the agent it shows; maximize preserves the shelf height and both reset on close |
| The draft | In `ViewState`, keyed by agent, so looking away and returning finds it |
| Bindings | The routing spec's key grammar: `Enter`, `Escape`, `Ctrl-F`, `Ctrl-Shift-↑`/`↓`, and a drag on the bottom edge with pointer capture (INV-4) |

## Failure modes

| Situation | Response |
| --- | --- |
| A window command with nothing open | A no-op that advances no revision |
| The looked-at agent leaves the roster | The panel says so rather than painting an empty box |
| A resumed child's journal is missing, locked or invalid | The roster row remains selectable and its conversation shows one explicit history warning; browsing starts no work |
| Going to a request from the primary itself | No window opens and any open one closes; the keyboard goes to the primary |
| No sub-agents yet | No rail is registered; its column goes to the conversation |
| A drag that began on the body, not the edge | Moves nothing; a grab is recorded at press or not at all |
| A press on another surface while a grab is held | Clears the grab |
| A height dragged past the guarantee | Clamped to it |
| A conversation region too small for two surfaces | Maximized, not a sliver |
| A window too short for a conversation and an input | Navigation-only (INS-7); it still holds focus, and giving the rows back gives the input back |
| Closing while a selection was made in the window | The selection goes with it (SEL-3), and so do the maximize and the dragged height |
| Focus preferring an unregistered window | Falls back to the first ring stop, and reclaims focus if it returns (SURF-5) |

## Evidence

[Named proofs](../evidence/inspector.md), one row an invariant.
