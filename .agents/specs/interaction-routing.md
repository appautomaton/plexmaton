# Spec — Interaction routing

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Translation from terminal events to typed intents, pointer capture, and the Escape ladder |
| Depends on | The locked input decisions in [`ui-ux.md`](../ui-ux.md) §input, and its §input and event-routing contract |
| Proven by | `plexmaton-tui::{router,workspace}` tests |

## Invariants

**INV-1 — Total, single-valued translation.** One terminal event produces at most one outcome:
exactly one `TuiIntent`, or an `Ignored` value naming why nothing happened. The router translates
and never reduces: which surface gains focus on a press, and what a submission does, are the
reducer's. Rejected: per-widget event handling, and a silent fallthrough that makes a dead key
indistinguishable from a routing defect.

**INV-2 — Printable keys follow the cursor.** A printable key produces a text intent if and only if
a text input holds keyboard focus; under navigation focus the same key is a command or unbound.
There is no third case, because there is never more than one cursor.

**INV-3 — Hover never redirects input.** Bare motion changes only the visual target under the
topmost surface, takes no capture, and repeats for free. Wheel motion changes only the topmost
eligible viewport; an immovable surface is transparent and an exhausted one still consumes the
event. Neither path changes focus, selection, or semantic entries (`ui-ux.md` §nested scrolling).

**INV-4 — Capture wins for the drag gesture.** Captured button and motion events stay on their
surface regardless of position; hit testing is not consulted. Wheels keep hover routing. Terminal
focus loss pauses motion but preserves capture for a later drag.

**INV-5 — Capture is released exactly once.** A release or a cancel clears capture; a second
release produces `Ignored::NoCapture`, never a second drag intent.

**INV-6 — The Escape ladder resolves one layer per press.** Cancel active capture first, then
resolve the focused input selection or topmost overlay. A focused primary approval returns focus
to its composer while its card stays visible. Inside conversations, clear selection before closing
the inspector. `Escape` never quits.

**INV-7 — Quit is a timed chord, and `Ctrl-C` never quits.** `Ctrl-D` asks, then leaves only on a
second press before its one-second monotonic deadline; expiry clears the question, and unrelated
terminal events leave the deadline alone. `Ctrl-C` clears the resolved conversation's non-empty
draft without an interrupt, or interrupts that conversation when its draft is empty; either path
withdraws the quit question. No bare key quits; rejected alternatives are in `ui-ux.md` §input.

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
navigation focus; they are absent from the Drawer, and nothing global names them. A button
activates only on a matching press/release without a drag; stale or unavailable targets do nothing.

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
a drag keeps it, and a release or `Escape` gives it back (INV-4, INV-5).

### Key grammar

The primary composer's visible [skill picker](./skill-picker.md) owns Up/Down, Tab/Enter and Esc
under SKP-3; completing a name edits the draft without submitting it. Outside that surface the
ordinary bindings below apply.

| Input | Navigation focus | Text focus |
| --- | --- | --- |
| `Ctrl-D` | Quit chord: arm one second, then leave on a timely second press | The same |
| `Ctrl-C` | Clear a non-empty draft; otherwise interrupt its conversation | The same |
| `Esc` | Escape ladder | Escape ladder |
| `Ctrl-P` | Pull the Drawer open and focus it (DRW-1) | The same |
| `Tab` / `Shift-Tab` | Cycle focus forward / backward | Cycle focus forward / backward |
| `q` | Unbound | Insert `q` |
| `↑` / `k`, `↓` / `j` | Move selection, which in the list opens or moves the second window (INS-1) | Unbound |
| `Enter` | Enter the second window | Submit |
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
| `Shift-Enter`, `Alt-Enter` | Unbound | Newline |

Control chords are never text (INV-2); reducers decide whether their target exists. A focused
decision region uses arrows to choose, `Enter` to decide and `Ctrl-O` to disclose. Matching button
press/release decides the exact displayed request. Primary approval uses `Esc` to return to input
without answering or hiding the card; a background modal closes. Allow and remember… enters a
scope/lifetime review; `Esc` there returns to the decision step. PER-5 owns producer confirmation.

The Drawer owns typing and editing while open (DRW-3): `↑` / `↓` chooses, `Enter` opens the row,
and `Esc` returns one layer. Its filter uses the caret contract in COM-1. The Permissions page
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

| Invariant | Proven by |
| --- | --- |
| INV-1, INV-11 | `retry_click_keyboard_and_drag_cancellation_share_one_action`, `retry_frames_keep_actions_with_the_failed_request_at_three_widths` with the `retry-*` frames |
| INV-1 | `every_terminal_event_is_translated_or_named_as_ignored` |
| INV-2 | `printable_keys_follow_the_cursor`, `the_inspector_grammar_is_the_same_under_both_focus_modes_except_enter`, `ctrl_o_is_the_same_disclosure_intent_under_both_focus_modes` |
| INV-3 | `pointer_motion_routes_a_hover_without_capture_or_focus`, `hover_changes_only_the_foldable_rows_appearance_and_repeating_it_costs_nothing`, `wheel_routes_by_hover_and_never_changes_focus`, `the_wheel_falls_through_what_cannot_scroll_and_stops_at_what_is_merely_exhausted`, `a_wheel_over_the_workspace_with_nothing_to_scroll_says_so` |
| INV-4 | `capture_keeps_the_drag_on_its_surface`, `wheel_is_not_captured_by_a_drag`, `focus_loss_suspends_motion_without_releasing_capture`, `dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle` |
| INV-5 | `capture_is_released_exactly_once` |
| INV-6 | `escape_resolves_one_layer_per_press`, `selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation` |
| INV-7 | `quit_is_explicit_and_unreachable_while_typing`, `the_quit_chord_confirms_only_inside_its_one_second_window`, `the_quit_deadline_expires_once_and_costs_one_frame`, `ctrl_c_clears_a_draft_or_interrupts_but_never_does_both`, `ctrl_c_names_the_conversation_it_interrupts`, `production_mapping_preserves_message_steering_interrupt_and_approval` |
| INV-8 | `a_modifier_does_not_make_a_pointer_event_disappear`, `dragging_across_a_conversation_selects_and_copies_what_it_crossed` |
| INV-9 | `resize_is_an_intent` |
| INV-10 | `an_arrow_moves_the_rail_and_scrolls_everything_else`, `the_queues_cursor_moves_without_touching_the_agent_selection`, `approval_keys_stay_inside_the_blocking_surface` |
