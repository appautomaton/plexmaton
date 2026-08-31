# Spec — Interaction routing

| Field | Value |
| --- | --- |
| Status | Implemented for the Phase 00 grammar; consumers arrive with later steps |
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

**INV-2 — Printable keys follow the cursor.** A printable key produces a text intent if and only if
a text input holds keyboard focus. Under navigation focus the same key is a command or unbound.
There is no third case, because there is never more than one cursor.

**INV-3 — Wheel events never change focus.** A wheel event resolves against the surface under the
pointer and produces a scroll intent. It can never produce a focus- or selection-changing intent.

**INV-4 — Capture wins for the drag gesture.** While pointer capture is held, button and motion
events route to the capturing surface regardless of position, and hit testing is not consulted.
Wheel events are excluded: they keep hover routing, because a drag on one surface must not freeze
scrolling everywhere else.

**INV-5 — Capture is released exactly once.** A release or a cancel clears capture. A second
release produces `Ignored::NoCapture`, never a second drag intent.

**INV-6 — The Escape ladder resolves one layer per press.** In order: cancel an active drag, then
dismiss the topmost dismissible layer, then nothing. `Escape` never quits.

**INV-7 — Quit is explicit and unreachable while typing.** `Ctrl-C` quits from any focus. `q` quits
only under navigation focus with no dismissible layer open. A printable `q` typed into a text input
is text.

**INV-8 — Terminal-native selection has a modifier escape hatch.** A pointer event carrying `Shift`
is not routed to any surface, so the terminal's own selection keeps working over an owned screen.

**INV-9 — Geometry is an intent.** A terminal resize produces an intent like any other event, so no
other component needs to observe raw terminal events to stay correct.

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
| `q` | Quit when nothing is dismissible | Insert `q` |
| `↑` / `k`, `↓` / `j` | Move selection | Unbound for now |
| Printable character | Unbound unless bound above | Insert |
| `Backspace` | Unbound | Delete backward |
| `Enter` | Unbound | Submit |
| `Shift-Enter`, `Alt-Enter` | Unbound | Newline |

Key *release* events are ignored, so a terminal reporting press and release does not act twice.

## Failure modes

| Situation | Response |
| --- | --- |
| Unbound key | `Ignored::Unbound`; never a silent default action |
| Pointer press outside every registered surface | `Ignored::OutsideWorkspace`; capture is not taken |
| Drag or release with no capture held | `Ignored::NoCapture` |
| `Escape` with no drag and nothing dismissible | `Ignored::NothingToDismiss`, not a quit |
| Wheel over no surface | `Ignored::OutsideWorkspace` |
| Pointer event with `Shift` held | `Ignored::TerminalSelection` per INV-8 |
| Key release or repeat frames | Release ignored; repeat treated as a press |

## Out of scope

- **Which viewport a scroll intent moves, and boundary behaviour.** The no-propagation rule is
  locked in [`ui-ux.md`](../roadmap/ui-ux.md); the mechanism arrives with per-surface viewports.
- **Clipping, modality, and focusability on `SurfaceTree`.** Delivery step 2. The router already
  reads the tree, so it inherits those rules when they land.
- **Cursor movement inside a text input.** It arrives with the composer, which owns its own editing
  model.
- **Shelf resize and inspector bindings.** `Ctrl-Shift-↑/↓` is locked in `ui-ux.md`, but an intent
  for a surface that does not exist yet would be a claim without a consumer.
- **What an intent does.** The reducer owns that.

## Evidence

| Invariant | Proven by |
| --- | --- |
| INV-1 | `every_terminal_event_is_translated_or_named_as_ignored` |
| INV-2 | `printable_keys_follow_the_cursor` |
| INV-3 | `wheel_routes_by_hover_and_never_changes_focus` |
| INV-4 | `capture_keeps_the_drag_on_its_surface`, `wheel_is_not_captured_by_a_drag` |
| INV-5 | `capture_is_released_exactly_once` |
| INV-6 | `escape_resolves_one_layer_per_press` |
| INV-7 | `quit_is_explicit_and_unreachable_while_typing` |
| INV-8 | `shift_leaves_pointer_events_to_the_terminal` |
| INV-9 | `resize_is_an_intent` |

Consumers are a separate question from the grammar. `Quit`, `MoveSelection`, and `TerminalResized`
are consumed by the executable today; `CycleFocus`, `Dismiss`, `Scroll`, `Pointer`, and `Text` are
produced and tested but have no reducer until steps 2 to 4 of the phase delivery sequence. That gap
is deliberate and recorded in
[phase 00](../roadmap/phase-00-experience-skeleton.md) rather than hidden.
