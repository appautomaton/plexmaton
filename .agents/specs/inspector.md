# Spec — The second window

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What the second window shows, where it goes, and what opening, entering, resizing, and closing it do |
| Depends on | [surface-model](./surface-model.md) SURF-3 and SURF-5; the Escape ladder in [interaction-routing](./interaction-routing.md) INV-6; the shelf rules in [`ui-ux.md`](../ui-ux.md) §shelf |
| Proven by | `plexmaton-tui::layout::inspector`, `::state::inspector`, and `::workspace` tests |

The code calls this surface `Inspector` (ui-ux §product vocabulary). On screen it is the second
window: the user is talking to one agent and looking at another.

## Invariants

**INS-1 — The window is the selection.** The list holds only the sub-agents, and is registered only
while it holds one: a workspace that has delegated nothing gives the rail's column to the
conversation. Selecting an agent, by arrow, click, or going to its request, opens its conversation
over or beside the primary's, and `Escape` clears the selection and closes it. Nothing stores which
agent is open apart from the selection, so no conversation is on screen twice; there is no pin and
no follow (ui-ux §shelf). Rejected: storing the open window beside the selection, with pin and
follow; the default path put one conversation on screen twice.

**INS-2 — Ten readable rows stay beneath the window, or the window takes the region outright.**
There is no third outcome where a shelf and a squeezed conversation share a region too small for
both. A terminal with fewer than ten rows before anything opened may have none covered.

**INS-3 — Presentation is derived from size, never stored.** Shelf, column, or maximized is chosen
per frame from the layout class and the user's maximize; changing it changes no identity, scroll
position, or focus.

**INS-4 — Entering is explicit; closing gives focus back.** Looking at an agent leaves the keyboard
in the list, so the arrows keep moving through it; `Enter` moves it into the window. Closing
returns focus to the conversation only when the window held it. Rejected: focusing on
look, which stops the arrows.

**INS-5 — The window's input exists only while the window holds focus** (ui-ux §input). It takes a
strip off the bottom of the window's own rectangle, never off the conversation's guarantee, and
while it is active the primary composer collapses to one row that stays clickable and stays a focus
stop. A wheel over the strip addresses that window's conversation viewport. A rectangle with no room
for both keeps the conversation and shows no input.

**INS-6 — What the window shows is a conversation.** The looked-at agent's, through the same cache
and reading position the main conversation uses (TR-1, TR-3, TR-5), keyed by agent. Text, tools,
mail and artifacts share its first-appearance order and one viewport; there is no parallel detail
surface or regrouped order. Rejected: composing status and an artifact index into the window now,
which needs sub-region scroll ownership and an expand model the transcript lacks; Phase 03's.

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
| Fallback | Below eighteen rows of conversation the presentation is maximized: a shelf of two borders and six lines is not worth being one |
| Layering | The one surface above the base layer, drawn inside the conversation's border so the conversation keeps its rectangle, title and reading position. The cells beneath are cleared first |
| The column | At ultrawide, out of the conversation's width, never the agent column's (ui-ux §layout classes) |
| Maximize and dragged height | In `state::inspector`: they belong to the window, not the agent it shows; maximize preserves the shelf height and both reset on close |
| The draft | In `ViewState`, keyed by agent, so looking away and returning finds it |
| Bindings | The routing spec's key grammar: `Enter`, `Escape`, `Ctrl-F`, `Ctrl-Shift-↑`/`↓`, and a drag on the bottom edge with pointer capture (INV-4) |

## Failure modes

| Situation | Response |
| --- | --- |
| A window command with nothing open | A no-op that advances no revision |
| The looked-at agent leaves the roster | The panel says so rather than painting an empty box |
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

| Invariant | Proven by |
| --- | --- |
| INS-1 | `the_rail_is_registered_only_when_there_is_a_roster`, `the_window_floats_over_the_primary_and_escape_closes_it`, `clicking_an_agent_in_the_list_selects_it_and_opens_its_window`, `the_journey_reaches_two_agents_without_losing_the_first`, `the_journey_keeps_a_second_agent_on_screen_and_takes_a_request_without_being_interrupted` |
| INS-2 | `an_open_inspector_leaves_ten_readable_rows_or_takes_the_region_outright`, `a_dragged_height_is_clamped_rather_than_obeyed`, `dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle` |
| INS-3 | `presentation_follows_the_terminal_and_the_users_maximize`, `the_composer_survives_every_presentation`, `registered_surfaces_tile_the_terminal_without_gaps_or_overlap`, `the_presentation_survives_the_window_showing_another_agent` |
| INS-4 | `selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation`, `the_inspector_grammar_is_the_same_under_both_focus_modes_except_enter` |
| INS-5 | `the_inspector_takes_the_cursor_and_the_composer_keeps_one_row`, `only_a_press_on_the_bottom_edge_starts_a_resize`, `the_keyboard_moves_the_inspectors_edge_the_same_way_the_pointer_does`, `a_wheel_over_the_inspector_input_scrolls_that_inspectors_conversation` |
| INS-6 | `two_conversations_scroll_independently_and_neither_moves_the_other`, `an_inspected_conversation_keeps_its_own_reading_position_across_a_close_and_reopen`, `the_journey_reaches_two_agents_without_losing_the_first`, `a_conversation_drawn_at_two_widths_measures_correctly_at_both` |
| INS-7 | `an_inspector_too_short_for_its_input_takes_no_typing_and_no_cursor`, `an_inspector_splits_for_its_input_only_when_both_still_fit` |
| INS-8 | `resizing_a_maximized_inspector_preserves_the_shelf_height`, `derived_maximized_and_column_inspectors_have_no_resize_edge` |
