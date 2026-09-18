# Spec — Interaction routing

| Field | Value |
| --- | --- |
| Status | Implemented and wired |
| Owns | Translation from terminal events to typed intents, pointer capture, and the Escape ladder |
| Depends on | The locked input decisions in [`ui-ux.md`](../ui-ux.md) §input, and its §input and event-routing contract |
| Proven by | `plexmaton-tui::{router,workspace}` tests, CLI composition tests and PTY journeys |

## Invariants

**INV-1 — Total, single-valued translation.** One terminal event produces at most one outcome:
exactly one `TuiIntent`, or an `Ignored` value naming why nothing happened. The router translates
and never reduces: which surface gains focus on a press, and what a submission does, are the
reducer's. Rejected: per-widget event handling, and a silent fallthrough that makes a dead key
indistinguishable from a routing defect.

**INV-2 — Printable keys follow the cursor.** A printable key produces a text intent if and only if
a text input holds keyboard focus; under navigation focus the same key is a command or unbound.
There is no third case, because there is never more than one cursor.

**INV-3 — Pointer and keyboard share the focused menu's choice.** Actual movement over an enabled
row in the focused approval, Drawer or composer menu updates its one selected choice without
activating it. Repeated coordinates cannot override keyboard navigation; hover outside that menu
never changes focus or transcript selection. Wheel routing retains `ui-ux.md` §nested scrolling.

**INV-4 — Capture wins for the drag gesture.** Captured button and motion events stay on their
surface regardless of position; hit testing is not consulted. Wheels keep hover routing. Terminal
focus loss pauses motion but preserves capture for a later drag.

**INV-6 — The Escape ladder resolves one layer per press.** Cancel active capture first, then
resolve the focused input selection or topmost overlay. A focused primary approval returns focus
to its composer while its card stays visible. Inside conversations, clear selection before closing
the inspector. `Escape` never quits.

**INV-7 — Quit is a timed chord, and `Ctrl-C` never quits.** `Ctrl-D` asks, then leaves only on a
second press before its one-second monotonic deadline; expiry clears the question, and unrelated
terminal events leave the deadline alone. `Ctrl-C` clears the resolved conversation's non-empty
draft without an interrupt, or interrupts that conversation when its draft is empty; either path
withdraws the quit question. The composition routes the root to its live runtime and a child to the
collaboration owner; a typed child refusal is never retried against the root. No bare key quits;
rejected alternatives are in `ui-ux.md` §input.

**INV-8 — No modifier is reserved: the escape hatch is the terminal's.** A terminal that bypasses
mouse reporting keeps the gesture on whichever modifier it chose, so an event that *arrives*
carrying one was forwarded on purpose and routes like any other. Copyability over an owned screen
is SEL-1's.

**INV-9 — Geometry is an intent.** A terminal resize produces an intent like any other event, so no
other component observes raw terminal events.

**INV-10 — A navigation key means "move inside what holds focus".** An arrow chooses an agent only
in the rail, moves the queue's cursor only in the queue, and everywhere else scrolls the surface the
user is in, which is the wheel's keyboard equivalent (ui-ux §user control).

**INV-11 — Retry is message-local.** Retry and Edit & retry are actions on an eligible failed
message (JRN-8), invoked by their inline buttons or `r` / `e` while the primary transcript has
navigation focus; they are absent from the Drawer, and nothing global names them. A button, on
any surface with rows, activates only on a matching press/release without a drag; stale or
unavailable targets do nothing. One press slot serves every such surface, so a press on one
surface cannot activate a release on another.

The Drawer's chord, geometry and pages are [drawer](./drawer.md) DRW-1 to DRW-4.

PER-8 owns the Permissions page's rule review: Up/Down and the wheel scroll complete scopes with fixed
controls; Enter continues to a separate confirmation, and Esc returns one page.

## Model

```text
crossterm::Event ──▶ Router::translate(event, RouterContext) ──▶ Routed
                                                                  ├─ Intent(TuiIntent)
                                                                  └─ Ignored(reason)
```

`RouterContext` is a read-only snapshot of input mode, focus, dismissible state, selection, and the
last frame's `SurfaceTree`. The router mutates only its capture: a left press on a surface takes it,
a drag keeps it, and a release or `Escape` gives it back (INV-4); releasing twice gives back
nothing, which `capture_is_released_exactly_once` holds.

### Key grammar

A focused approval sheet accepts its displayed `1`–`2` or `1`–`3` choices, plus Up/Down and Enter.
Modified digits, reported digit/Enter key repeats, unavailable rows and submitting choices do nothing; digits
remain text under composer focus. Decisions bind to the request, stage and offered scopes in the
last successfully delivered frame; a replacement or newly opened scope must be painted first. Numbered remembered grants use the same visible-scope guard as
Enter and clicking (PER-10). Mouse movement over the focused card selects the choice without deciding.
[Command inspection](./approval-inspection.md) owns command-summary clicks, Ctrl-O, modal copy and
close. Other tool details retain Ctrl-O's inline expansion.

The `/effort` selector uses EFF-2: Left/Right or Up/Down skip disabled stops, pointer movement or release
previews a selectable level, Enter confirms, and Escape cancels a held press before closing.

The primary composer's visible [composer menu](./composer-menu.md) owns Up/Down, Tab/Enter and Esc
under SKP-3; completing a name edits the draft without submitting it. Outside that surface the
ordinary bindings below apply.

The [conversation tree](./conversation-tree.md) owns a modal grammar under TRE-1/TRE-6: `/tree`
and `/rewind` open the same view; Up/Down or j/k move one cursor, Home/End choose an edge,
Enter rewinds an eligible message or selects a branch, b switches messages/branches, f toggles the selected
message's displayed descendants, r refreshes, y/Ctrl-Y copies exact source, l edits a message label,
n renames a branch and x opens its retirement confirmation. Mouse movement and row clicks select
without navigating; clicking any cell of `[+]` expands and `[−]` collapses.
Intermediate ineligible tool rows have no control in this list; read-only rows do nothing on Enter.
Connector and collapsed-summary lines are inert; movement advances by message, not physical row. Enter and mutation shortcuts accept
presses, not repeats. A child text prompt uses ordinary single-line editing; Enter submits and
Escape cancels that child before closing browsing. Escape first cancels any held pointer capture;
the close button dismisses the whole tree. Hidden composer shortcuts and paste are blocked.
Ctrl-P still opens the Drawer above the tree, Ctrl-D remains global, and Ctrl-C withdraws its quit
question without clearing or interrupting the hidden conversation. An admitted write continues
after dismissal; acknowledgement never reopens the view or steals focus from the Drawer.
Ctrl-B is owned and ignored by the Drawer or tree while either covers the workspace, so a retained
Agents navigator and the overlay's exact return focus cannot change behind the visible surface.
Below the minimum terminal size, Escape still dismisses the retained tree, but editing and
navigation are disabled. Native proof and rendered review status remain in the tree spec.

Narrow Agents has one modal grammar: `↑`/`↓` or `j`/`k` moves its temporary cursor, `Enter`
commits and enters the row, and `Escape` or `Ctrl-B` restores the exact prior focus. Inspector,
selection, retry and text commands cannot reach the retained conversation. `Ctrl-C` may withdraw a
quit question but cannot clear or interrupt a hidden conversation. The collapsed `Agents ^B`
handle and each agent row require a matching press and release; drag, resize, focus loss or target
change cancels activation (INV-11).

| Input | Navigation focus | Text focus |
| --- | --- | --- |
| `Ctrl-D` | Quit chord: arm one second, then leave on a timely second press | The same |
| `Ctrl-C` | Clear a non-empty draft; otherwise interrupt its conversation | The same |
| `Esc` | Escape ladder | Escape ladder |
| `Ctrl-P` | Pull the Drawer open and focus it (DRW-1) | The same |
| `Ctrl-B` | Toggle the roster column, or enter/leave the Narrow full-region navigator | The same |
| `Tab` / `Shift-Tab` | Cycle focus forward / backward | Cycle focus forward / backward |
| `q` | Unbound | Insert `q` |
| `↑` / `k`, `↓` / `j` | Move the list selection; Narrow Agents moves an uncommitted cursor (INS-1) | `↑` / `↓` move the caret one painted row and the window follows (COM-2); `k` / `j` insert |
| Wheel over the composer | Nothing scrollable | Walk the draft one row per notch (COM-2) |
| `Enter` | Commit the focused Agents row and enter the second window | Submit |
| `Ctrl-F` | Maximize the second window | Maximize the second window |
| `Ctrl-Shift-↑` / `Ctrl-Shift-↓` | Shrink, grow the second window | Shrink, grow the second window |
| `Shift-↑` / `Shift-↓` | Extend the selection; with none, select the newest entry (SEL-1) | The same |
| `Ctrl-O` | Toggle retained detail for the selection's moving end (ENT-4) | The same |
| `Ctrl-Y` | Copy (SEL-4) | Copy |
| `r` / `e` | Retry / Edit & retry on an eligible failure in the primary transcript | Insert text |
| Printable character | Unbound unless bound above | Insert |
| `Backspace` | Unbound | Delete backward |
| `Left` / `Right`, `Home` / `End` | Unbound | Move by grapheme or to the logical line's edge |
| `Ctrl/Alt-Left` / `Ctrl/Alt-Right` | Unbound | Move by word |
| `Ctrl-A` / `Ctrl-E`, `Alt-B` / `Alt-F` | Unbound | Line-edge or word motion |
| `Delete`, `Ctrl-W`, `Ctrl-U` / `Ctrl-K` | Unbound | Delete forward, previous word, or to the line's edge |
| `Ctrl-J`, `Shift-Enter`, `Alt-Enter` | Unbound | Newline |
| `Alt-↑` | Unbound | Take the most recent waiting message into the empty primary composer; an occupied draft leaves the queue unchanged (IQU-4) |

Control chords are never text (INV-2); reducers decide whether their target exists. A focused
decision region uses arrows to choose, `Enter` to decide and `Ctrl-O` to disclose. Matching button
press/release decides the exact displayed request. Primary approval uses `Esc` to return to input
without answering or hiding the card; a background modal closes. Allow and remember… enters a
scope/lifetime review; `Esc` there returns to the decision step. PER-5 owns producer confirmation.

The Drawer owns typing and editing while open (DRW-3): `↑` / `↓` chooses, `Enter` opens the row,
and `Esc` returns one layer. Modified `Enter` and `Ctrl-J` are unbound in the Drawer. Its filter
uses the caret contract in COM-1. The Permissions page
holds revisioned grant controls (PER-7): arrows or drawn-row clicks select a setting or grant,
`Enter` reviews it, and a second confirmation applies it. Back starts selected; `Esc` returns from
review, then to the list. Loading/submission accepts no duplicate mutation. The Configuration page
is navigated, never typed into: `↑` / `↓` or `k` / `j` scroll its values (DRW-4).

| Fact | Value |
| --- | --- |
| Status line | The last row, chrome under every pane: the working directory at rest, replaced by an owned question until its transition resolves it |

## Failure modes

| Situation | Response |
| --- | --- |
| Unbound key | `Ignored::Unbound`; never a silent default action |
| Pointer press outside every registered surface | `Ignored::OutsideWorkspace`; capture is not taken |
| Drag or release with no capture held | `Ignored::NoCapture` |
| `Escape` with nothing on the ladder | `Ignored::NothingToDismiss`, not a quit |
| `Ctrl-C` with nothing to clear | Interrupt the focused conversation; the status line remains at rest |
| Wheel with nothing scrollable beneath | `Ignored::NothingScrollable`, a different fact from being outside the workspace, which is `Ignored::OutsideWorkspace` |
| Bare pointer motion outside the workspace | A hover intent with no target, clearing prior feedback |
| `Event::Paste` | Insert into the focused input without submission; ignore under navigation focus or a blocking non-text surface |
| Key release or repeat | Release ignored; repeat treated as a press |

## Evidence

[Named proofs](../evidence/interaction-routing.md), one row an invariant.
