# Spec — Interaction routing

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Translation from terminal events to typed intents, pointer capture, and the Escape ladder |
| Depends on | The locked input decisions in [`ui-ux.md`](../roadmap/ui-ux.md) |
| Proven by | `plexmaton-tui::router` tests; see the evidence table |

## Purpose

One component turns terminal events into typed intents. Without it every later interaction gets
wired ad hoc into the widget that happens to be on screen, and the rules that are already locked —
exactly one cursor, hover-routed scrolling, capture during drag, an Escape ladder — end up
re-decided independently in each widget.

The router is a translator, not a reducer. It answers "what did the user ask for", never "what
should change". Which surface gains focus on a press, and what a submitted message does, belong to
the reducer.

## Invariants

**INV-1 — Total, single-valued translation.** One terminal event produces at most one outcome:
either exactly one `TuiIntent`, or an `Ignored` value naming why nothing happened. There is no
branch that discards an event without saying so.

Rejected: per-widget event handling, and a silent fallthrough that makes a dead key
indistinguishable from a routing defect.

**INV-2 — Printable keys follow the cursor.** A printable key produces a text intent if and only if
a text input holds keyboard focus. Under navigation focus the same key is a command or unbound.
There is no third case, because there is never more than one cursor.

**INV-3 — Wheel events never change focus, and resolve by eligibility.** A wheel event produces a
scroll intent and can never produce a focus- or selection-changing one. Its target is the topmost
surface under the pointer whose viewport *can move*: a surface with nothing to scroll is transparent
and the event reaches what is beneath it, while a surface merely at its boundary is still the target
and consumes it (ui-ux §nested scrolling). "Cannot scroll" and "scrolled to the end" are deliberately different, because
a gesture whose target changes with scroll position destroys the spatial memory it depends on.

**INV-4 — Capture wins for the drag gesture.** While pointer capture is held, button and motion
events route to the capturing surface regardless of position, and hit testing is not consulted.
Wheel events are excluded: they keep hover routing, because a drag on one surface must not freeze
scrolling everywhere else.

**INV-5 — Capture is released exactly once.** A release or a cancel clears capture. A second
release produces `Ignored::NoCapture`, never a second drag intent.

**INV-6 — The Escape ladder resolves one layer per press.** In order: cancel an active drag, then
dismiss the topmost dismissible layer, then nothing. `Escape` never quits.

**INV-7 — Quit is explicit and unreachable while typing.** `Ctrl-C` quits from any focus, and no
bare key quits from any. A printable `q` is text under a cursor and unbound elsewhere. Rejected: `Escape` as quit, which
the reflex that closes an overlay would trigger one press later; and a bare `q`, which ended the
session the first time a message was typed one `Tab` too early, because focus starts on a
navigation surface and moves without the screen saying so.

**INV-8 — Terminal-native selection has a modifier escape hatch.** A pointer event carrying `Shift`
is not routed to any surface, so the terminal's own selection keeps working over an owned screen.

**INV-9 — Geometry is an intent.** A terminal resize produces an intent like any other event, so no
other component needs to observe raw terminal events to stay correct.

**INV-10 — A navigation key means "move inside what holds focus".** Which list an arrow moves is a
fact about the focused surface, not a global binding: it chooses an agent only in the rail, moves the
queue's cursor only in the queue, and everywhere else scrolls the surface the user is in. That last
case is also what gives the wheel the keyboard equivalent `ui-ux.md` §user control requires of every
mouse gesture.

## Model

```text
crossterm::Event ──▶ Router::translate(event, RouterContext) ──▶ Routed
                                                                  ├─ Intent(TuiIntent)
                                                                  └─ Ignored(reason)
```

### Ownership

| Fact | Owner | Why not elsewhere |
| --- | --- | --- |
| Pointer capture | `Router` | Input-layer state; nothing else has a use for it |
| Keyboard focus | View state, passed in as `RouterContext::focus` | A second copy in the router is a second source of truth |
| Surface geometry and z-order | `SurfaceTree`, borrowed by `RouterContext` | Hit testing must read the same tree the renderer laid out |
| Whether a dismissible layer is open | View state, passed in as `RouterContext::dismissible` | Same reason as focus |
| Which surface holds focus | View state, passed in as `RouterContext::focused` | `focus` says whether a cursor exists; this says where the user is, and INV-10 needs both |
| Which layer `Escape` resolves next | View state | The router asks only whether *anything* is there (`dismissible`, `selecting`); ranking the rungs in two places is how the two drift |

`RouterContext` is a read-only snapshot. The router mutates only its own capture.

### Capture state machine

```text
        Down(left) on a surface          Up | Escape
Idle ─────────────────────────▶ Captured ───────────▶ Idle
                                   │
                                   └── Drag ──▶ Captured (Pointer::Drag)
```

`Down` outside every surface leaves the state `Idle` and reports `Ignored::OutsideWorkspace`.

### Key grammar

| Input | Navigation focus | Text focus |
| --- | --- | --- |
| `Ctrl-C` | Quit | Quit |
| `Esc` | Escape ladder | Escape ladder |
| `Tab` / `Shift-Tab` | Cycle focus forward / backward | Cycle focus forward / backward |
| `q` | Unbound | Insert `q` |
| `↑` / `k`, `↓` / `j` | Move selection, which in the list opens or moves the second window (INS-1) | Unbound for now |
| `Enter` (see below) | Enter the second window | Submit |
| `Ctrl-F` | Maximize the second window | Maximize the second window |
| `Ctrl-Shift-↑` / `Ctrl-Shift-↓` | Shrink, grow the second window | Shrink, grow the second window |
| Printable character | Unbound unless bound above | Insert |
| `Backspace` | Unbound | Delete backward |
| `Enter` | Unbound | Submit |
| `Shift-Enter`, `Alt-Enter` | Unbound | Newline |

The second-window chords resolve *before* keyboard focus is consulted, which is what makes them
reachable while the window's own input holds the cursor — a control chord is never text (INV-2).
`Enter` is the deliberate exception and the reason the others are chords: under a cursor it submits,
so entering cannot live there. They are translated whether or not a window is open; the router
says what was pressed, and whether there is anything to act on is the reducer's question.
[`inspector`](./inspector.md) owns what each one does.

Key *release* events are ignored, so a terminal reporting press and release does not act twice.

## Failure modes

| Situation | Response |
| --- | --- |
| Unbound key | `Ignored::Unbound`; never a silent default action |
| Pointer press outside every registered surface | `Ignored::OutsideWorkspace`; capture is not taken |
| Drag or release with no capture held | `Ignored::NoCapture` |
| `Escape` with no drag and nothing dismissible | `Ignored::NothingToDismiss`, not a quit |
| Wheel over the workspace with nothing scrollable beneath | `Ignored::NothingScrollable`, which is a different fact from being outside it |
| Wheel over no surface | `Ignored::OutsideWorkspace` |
| Pointer event with `Shift` held | `Ignored::TerminalSelection` per INV-8 |
| Key release or repeat frames | Release ignored; repeat treated as a press |

## Out of scope

- **What a viewport does once it has the intent** — how far it moves, and where it stops. The
  router resolves the target; [`surface-model`](./surface-model.md) owns the viewport.
- **Clipping and modality on `SurfaceTree`.** SURF-2 and SURF-4, neither owned by this phase.
- **Cursor movement inside a text input.** It arrives with the composer, which owns its own editing
  model.
- **What an inspector command does.** The bindings are above because translation is this file's
  job; the behaviour behind them is [`inspector`](./inspector.md).
- **What an intent does.** The reducer owns that.

## Evidence

| Invariant | Proven by |
| --- | --- |
| INV-1 | `every_terminal_event_is_translated_or_named_as_ignored` |
| INV-2 | `printable_keys_follow_the_cursor`, `the_inspector_grammar_is_the_same_under_both_focus_modes_except_enter` |
| INV-3 | `wheel_routes_by_hover_and_never_changes_focus`, `the_wheel_falls_through_what_cannot_scroll_and_stops_at_what_is_merely_exhausted`, `a_wheel_over_the_workspace_with_nothing_to_scroll_says_so` |
| INV-4 | `capture_keeps_the_drag_on_its_surface`, `wheel_is_not_captured_by_a_drag`, `dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle` |
| INV-5 | `capture_is_released_exactly_once` |
| INV-6 | `escape_resolves_one_layer_per_press`, `selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation` |
| INV-7 | `quit_is_explicit_and_unreachable_while_typing` |
| INV-8 | `shift_leaves_pointer_events_to_the_terminal` |
| INV-9 | `resize_is_an_intent` |
| INV-10 | `an_arrow_moves_the_rail_and_scrolls_everything_else`, `the_queues_cursor_moves_without_touching_the_agent_selection` |

Every intent has a consumer in the executable, and every one is reachable from a keyboard alone —
which the canonical journey exercises end to end in `plexmaton-tui::journey`.
