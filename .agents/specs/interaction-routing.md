# Spec — Interaction routing

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Translation from terminal events to typed intents, pointer capture, and the Escape ladder |
| Depends on | The locked input decisions in [`ui-ux.md`](../ui-ux.md) §input, and its §input and event-routing contract |
| Proven by | `plexmaton-tui::router` tests |

## Invariants

**INV-1 — Total, single-valued translation.** One terminal event produces at most one outcome:
exactly one `TuiIntent`, or an `Ignored` value naming why nothing happened. The router translates
and never reduces: which surface gains focus on a press, and what a submission does, are the
reducer's. Rejected: per-widget event handling, and a silent fallthrough that makes a dead key
indistinguishable from a routing defect.

**INV-2 — Printable keys follow the cursor.** A printable key produces a text intent if and only if
a text input holds keyboard focus; under navigation focus the same key is a command or unbound.
There is no third case, because there is never more than one cursor.

**INV-3 — Wheel events never change focus, and resolve by eligibility.** A wheel event produces a
scroll intent and never a focus- or selection-changing one. Its target is the topmost surface under
the pointer whose viewport can move: a surface with nothing to scroll is transparent, while one at
its boundary is still the target and consumes the event (ui-ux §nested scrolling).

**INV-4 — Capture wins for the drag gesture.** While pointer capture is held, button and motion
events route to the capturing surface regardless of position, and hit testing is not consulted.
Wheel events keep hover routing, so a drag on one surface does not freeze scrolling elsewhere.

**INV-5 — Capture is released exactly once.** A release or a cancel clears capture; a second
release produces `Ignored::NoCapture`, never a second drag intent.

**INV-6 — The Escape ladder resolves one layer per press.** In order: cancel an active drag, then
drop a selection, then dismiss the topmost dismissible layer, then nothing. `Escape` never quits.

**INV-7 — Quit is a chord pressed twice, and `Ctrl-C` never quits.** `Ctrl-D` asks on the first
press, in the status line, and leaves on the second in a row; any other key withdraws the question.
`Ctrl-C` is the interrupt: it clears the draft under the cursor, and with nothing to clear it points
at the chord. No bare key quits from any focus; a printable `q` is text under a cursor and unbound
elsewhere. Rejected: `Escape` as quit, which the reflex that closes an overlay would trigger one
press later; a bare `q`, which ended the session the first time a message was typed one `Tab` too
early; and `Ctrl-C` as a one-press exit, which took the session where a shell habit meant to take a
line.

**INV-8 — Terminal-native selection has a modifier escape hatch.** A pointer event carrying `Shift`
is routed to no surface, so the terminal's own selection keeps working over an owned screen.

**INV-9 — Geometry is an intent.** A terminal resize produces an intent like any other event, so no
other component observes raw terminal events.

**INV-10 — A navigation key means "move inside what holds focus".** An arrow chooses an agent only
in the rail, moves the queue's cursor only in the queue, and everywhere else scrolls the surface the
user is in, which is the wheel's keyboard equivalent (ui-ux §user control).

## Model

```text
crossterm::Event ──▶ Router::translate(event, RouterContext) ──▶ Routed
                                                                  ├─ Intent(TuiIntent)
                                                                  └─ Ignored(reason)
```

`RouterContext` is a read-only snapshot: whether a cursor exists, which surface holds focus,
whether a dismissible layer or a selection is open, and the `SurfaceTree` the last frame registered.
The router mutates only its own capture.

```text
        Down(left) on a surface          Up | Escape
Idle ─────────────────────────▶ Captured ───────────▶ Idle
                                   │
                                   └── Drag ──▶ Captured (Pointer::Drag)
```

### Key grammar

| Input | Navigation focus | Text focus |
| --- | --- | --- |
| `Ctrl-D` | Quit chord: ask, then leave on the second press in a row | The same |
| `Ctrl-C` | Interrupt: clear the draft, else point at the quit chord | The same |
| `Esc` | Escape ladder | Escape ladder |
| `Tab` / `Shift-Tab` | Cycle focus forward / backward | Cycle focus forward / backward |
| `q` | Unbound | Insert `q` |
| `↑` / `k`, `↓` / `j` | Move selection, which in the list opens or moves the second window (INS-1) | Unbound |
| `Enter` | Enter the second window | Submit |
| `Ctrl-F` | Maximize the second window | Maximize the second window |
| `Ctrl-Shift-↑` / `Ctrl-Shift-↓` | Shrink, grow the second window | Shrink, grow the second window |
| `Shift-↑` / `Shift-↓` | Extend the selection; with none, select the newest entry (SEL-1) | The same |
| `Ctrl-Y` | Copy (SEL-4) | Copy |
| Printable character | Unbound unless bound above | Insert |
| `Backspace` | Unbound | Delete backward |
| `Shift-Enter`, `Alt-Enter` | Unbound | Newline |

The quit, interrupt, second-window and selection chords resolve before keyboard focus is
consulted, so a control chord is never text (INV-2) and they reach the window while its own input
holds the cursor. `Enter`
is the exception, because under a cursor it submits. Chords translate whether or not a window is
open; whether there is anything to act on is the reducer's question. Key release events are
ignored, so a terminal reporting press and release does not act twice.

## Failure modes

| Situation | Response |
| --- | --- |
| Unbound key | `Ignored::Unbound`; never a silent default action |
| Pointer press outside every registered surface | `Ignored::OutsideWorkspace`; capture is not taken |
| Drag or release with no capture held | `Ignored::NoCapture` |
| `Escape` with nothing on the ladder | `Ignored::NothingToDismiss`, not a quit |
| `Ctrl-C` with nothing to clear | The status line says `Ctrl-D twice to quit`; nothing else changes |
| Wheel over the workspace with nothing scrollable beneath | `Ignored::NothingScrollable`, a different fact from being outside it |
| Wheel over no surface | `Ignored::OutsideWorkspace` |
| Pointer event with `Shift` held | `Ignored::TerminalSelection` (INV-8) |
| `Event::Paste` | Declined; nothing in the journey pastes |
| Key release or repeat | Release ignored; repeat treated as a press |

## Evidence

| Invariant | Proven by |
| --- | --- |
| INV-1 | `every_terminal_event_is_translated_or_named_as_ignored` |
| INV-2 | `printable_keys_follow_the_cursor`, `the_inspector_grammar_is_the_same_under_both_focus_modes_except_enter` |
| INV-3 | `wheel_routes_by_hover_and_never_changes_focus`, `the_wheel_falls_through_what_cannot_scroll_and_stops_at_what_is_merely_exhausted`, `a_wheel_over_the_workspace_with_nothing_to_scroll_says_so` |
| INV-4 | `capture_keeps_the_drag_on_its_surface`, `wheel_is_not_captured_by_a_drag`, `dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle` |
| INV-5 | `capture_is_released_exactly_once` |
| INV-6 | `escape_resolves_one_layer_per_press`, `selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation` |
| INV-7 | `quit_is_explicit_and_unreachable_while_typing`, `the_quit_chord_asks_once_and_leaves_on_the_second_press`, `ctrl_c_clears_the_draft_and_with_none_points_at_the_quit_chord` |
| INV-8 | `shift_leaves_pointer_events_to_the_terminal` |
| INV-9 | `resize_is_an_intent` |
| INV-10 | `an_arrow_moves_the_rail_and_scrolls_everything_else`, `the_queues_cursor_moves_without_touching_the_agent_selection` |

Every intent has a consumer in the executable and is reachable from a keyboard alone, which the
canonical journey exercises end to end in `plexmaton-tui::journey`.
