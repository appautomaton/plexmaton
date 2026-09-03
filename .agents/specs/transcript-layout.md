# Spec — Transcript layout

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | How tall a conversation is, which part of it a frame builds, and where its reader is |
| Depends on | [surface-model](./surface-model.md) SURF-5 for retained state; the wheel rules in [interaction-routing](./interaction-routing.md) INV-3 |
| Proven by | `plexmaton-tui::transcript`, `::render`, and `::state::scroll` tests |

## Invariants

**TR-1 — A height is measured once per entry, revision, disclosure, and width.** An entry's height comes from the
same wrapper that paints it (surface-model §viewports) and is recomputed only when that entry's
revision, open state, or the panel's width changes: a delta re-measures one entry, disclosure
re-measures one entry at each retained width, a resize re-measures each entry once, a tool lifecycle
update re-measures its one stable entry, and an unchanged frame re-measures none. Text, tool,
artifact and mail entries use the same ordered cache. Width is part of the key,
because a conversation changes
width when the second window opens beside it at ultrawide and comes back; a conversation keeps one
set of heights per width, bounded at two, evicting the width least recently measured.

**TR-2 — A frame builds only what it draws.** The lines a frame constructs are bounded by the
entries its viewport reaches, not by the conversation's length; off-screen entries contribute their
measured height and nothing else. A reached open tool contributes its complete bounded retained
detail to the parent paragraph so that one viewport, one wrapper, and one scroll position own it;
the producer's ENT-4 byte bound is the hard cap for that single entry.

**TR-3 — A reading position is an entry, not a row.** A parked conversation is anchored to a
transcript entry and a row inside it, so the same semantic fact stays on screen across a resize and
content arriving elsewhere does not move it. A width the reader left and returned to paints the frame it
had: resolving an anchor clamps the row for display without writing the clamp back, and only a
scroll rewrites the stored position. Both directions of the row-to-entry conversion resolve at the
width that produced the viewport being scrolled, carried out of the frame on the viewport itself.
Rejected: a row offset, which survives a resize as a number while naming different text; a
per-surface position, which loses A's place on returning from B (TR-5); and bounded overscan, which
a synchronous renderer cannot use.

**TR-4 — Following the tail is a state.** A viewport at its last row is following and stays at the
newest line as content arrives; scrolling away parks it and scrolling back to the end resumes it.
Deriving this from `offset == max_offset` loses it at the first delta. A conversation shorter than
its viewport is painted at the bottom of it, so a window floating over the top covers empty rows or
rows already read (ui-ux §shelf).

**TR-5 — A reading position belongs to the conversation.** Each agent's transcript keeps its own
position, so selecting another agent and returning restores where its reader was (SURF-5).

Disclosure is a reading action. Before an entry changes height, the viewport's current semantic top
anchor is parked and tail-follow is dropped; resolving that anchor after measurement keeps the
content the reader was on rather than jumping to the newly enlarged tail.

## Model

```text
ViewState (immutable to the renderer)        TranscriptMetrics (outlives the frame)
   TranscriptPosition per agent  ────────▶   offset_of ──▶ row
                                             window    ──▶ entries this frame builds
   wheel ◀──────────────────────────────────  anchor_at ◀── row
```

An anchor is `{ item, rows }`. The entry identity is the durable half; the row inside it is clamped
to that entry's height at the current width when resolved, so an entry that wrapped shorter is not
overshot. An anchor whose entry no longer exists resolves to the tail. The heights live in
`TranscriptMetrics`, held by the [frame loop](./frame-loop.md) and keyed by agent and width; the
width a surface's rows were measured at travels on `Viewport::content_width`. Semantic heights and
offsets use `usize`; at Ratatui's `u16` widget-scroll boundary, complete logical lines are removed
from the built prefix before the remaining scroll is narrowed.

| Change | Entries wrapped |
| --- | --- |
| A delta on one entry | 1 |
| A new entry | 1 |
| A tool lifecycle transition | 1 |
| Opening or closing one tool detail | 1 per retained width when that width is next drawn |
| A resize | every entry, once |
| Anything else, including scrolling | 0 |

Summing heights and checking each cached one is still valid visits every entry each frame; that is
arithmetic over a `Vec`, not wrapping, and `TranscriptMetrics::wrapped` and `::lines_built` are
what make the claim testable. What those walks cost is measured in [frame-loop](./frame-loop.md)
§cost.

## Failure modes

| Situation | Response |
| --- | --- |
| Panel too narrow to wrap into | An entry measures zero rows rather than dividing by a zero width |
| A conversation with no entries | A placeholder; there is nothing to virtualize |
| A conversation nothing has measured | No window and no anchor, so the wheel leaves it alone rather than parking it at a guess |
| An offset past the end of the content | An empty window. Drawing something arbitrary would hide the clamping error that produced it |
| Content taller than `u16::MAX` rows | Retain the full semantic offset and remove complete built-prefix lines until the terminal widget can express the remainder |
| An anchored entry that no longer exists | Resolves to the tail. Nothing removes an entry yet, so cache pruning has never run |

## Evidence

| Invariant | Proven by |
| --- | --- |
| TR-1 | `measurement_is_proportional_to_what_changed`, `a_tool_transition_remeasures_only_its_original_entry`, `ctrl_o_opens_the_selections_focus_entry_in_place_at_each_drawn_width`, `item_heights_sum_to_the_height_of_the_whole_conversation`, `compact_tool_entries_cost_one_wrap_at_any_history_length`, `opening_a_tool_entry_costs_one_wrap_and_not_its_history`, `the_resize_workload_re_measures_every_entry_exactly_once`, `two_widths_of_one_conversation_do_not_invalidate_each_other`, `a_run_of_widths_retains_only_the_last_two`, `a_conversation_drawn_at_two_widths_measures_correctly_at_both` |
| TR-2 | `a_virtualized_conversation_paints_what_the_whole_one_did`, `interleaved_text_and_tools_keep_their_positions_when_tools_finish_out_of_order`, `a_window_covers_the_viewport_and_starts_inside_the_item_it_lands_in`, `a_conversation_nothing_has_measured_has_no_window_and_no_anchor`, `maximum_newline_detail_and_the_entry_after_it_remain_reachable`, `frame_work_is_bounded_by_the_viewport_and_not_by_the_history`, `opening_a_tool_entry_costs_one_wrap_and_not_its_history`, `the_open_tool_frames_match_their_fixtures` |
| TR-3 | `an_anchor_round_trips_through_the_row_it_names`, `ctrl_o_opens_the_selections_focus_entry_in_place_at_each_drawn_width`, `a_conversation_resized_away_and_back_paints_the_frame_it_had`, `an_anchor_survives_a_width_change_and_a_row_number_does_not`, `a_resized_conversation_keeps_the_reader_on_the_same_message`, `a_wheel_notch_moves_the_conversation_the_same_distance_with_an_inspector_open` |
| TR-4 | `a_followed_viewport_moves_with_its_content_and_a_parked_one_does_not`, `a_conversation_scrolled_back_to_the_end_keeps_up_and_a_parked_one_stays_put`, `scrolling_clamps_to_the_content_and_reports_a_boundary_as_no_movement` |
| TR-5 | `each_conversation_keeps_its_own_reading_position` |
