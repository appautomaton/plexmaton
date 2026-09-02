# Spec — Transcript layout

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | How tall a conversation is, which part of it a frame builds, and where its reader is |
| Depends on | [surface-model](./surface-model.md) SURF-5 for retained state; the wheel rules in [interaction-routing](./interaction-routing.md) INV-3 |
| Proven by | `plexmaton-tui::transcript`, `::render`, and `::state::scroll` tests |

## Invariants

**TR-1 — A height is measured once per item, revision, and width.** An item's height comes from the
same wrapper that paints it (surface-model §viewports) and is recomputed only when that item's
revision or the panel's width changes: a delta re-measures one item, a resize re-measures each item
once, an unchanged frame re-measures none. Width is part of the key, because a conversation changes
width when the second window opens beside it at ultrawide and comes back; a conversation keeps one
set of heights per width, bounded at two, evicting the width least recently measured.

**TR-2 — A frame builds only what it draws.** The lines a frame constructs are bounded by the
viewport, not by the conversation's length; off-screen items contribute their measured height and
nothing else.

**TR-3 — A reading position is an item, not a row.** A parked conversation is anchored to a
transcript item and a row inside it, so the same text stays on screen across a resize and content
arriving elsewhere does not move it. A width the reader left and returned to paints the frame it
had: resolving an anchor clamps the row for display without writing the clamp back, and only a
scroll rewrites the stored position. Both directions of the row-to-item conversion resolve at the
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

## Model

```text
ViewState (immutable to the renderer)        TranscriptMetrics (outlives the frame)
   TranscriptPosition per agent  ────────▶   offset_of ──▶ row
                                             window    ──▶ items this frame builds
   wheel ◀──────────────────────────────────  anchor_at ◀── row
```

An anchor is `{ item, rows }`. The item is the durable half; the row inside it is clamped to that
item's height at the current width when resolved, so a message that wrapped shorter is not
overshot. An anchor whose item no longer exists resolves to the tail. The heights live in
`TranscriptMetrics`, held by the [frame loop](./frame-loop.md) and keyed by agent and width; the
width a surface's rows were measured at travels on `Viewport::content_width`.

| Change | Items wrapped |
| --- | --- |
| A delta on one item | 1 |
| A new item | 1 |
| A resize | every item, once |
| Anything else, including scrolling | 0 |

Summing heights and checking each cached one is still valid visits every item each frame; that is
arithmetic over a `Vec`, not wrapping, and `TranscriptMetrics::wrapped` and `::lines_built` are
what make the claim testable. What those walks cost is measured in [frame-loop](./frame-loop.md)
§cost.

## Failure modes

| Situation | Response |
| --- | --- |
| Panel too narrow to wrap into | An item measures zero rows rather than dividing by a zero width |
| A conversation with no items | A placeholder; there is nothing to virtualize |
| A conversation nothing has measured | No window and no anchor, so the wheel leaves it alone rather than parking it at a guess |
| An offset past the end of the content | An empty window. Drawing something arbitrary would hide the clamping error that produced it |
| Content taller than `u16::MAX` rows | Saturates; a viewport offset is a `u16` because that is what the terminal addresses |
| An anchored item that no longer exists | Resolves to the tail. Nothing removes an item yet, so cache pruning has never run |

## Evidence

| Invariant | Proven by |
| --- | --- |
| TR-1 | `measurement_is_proportional_to_what_changed`, `item_heights_sum_to_the_height_of_the_whole_conversation`, `the_resize_workload_re_measures_every_item_exactly_once`, `two_widths_of_one_conversation_do_not_invalidate_each_other`, `a_run_of_widths_retains_only_the_last_two`, `a_conversation_drawn_at_two_widths_measures_correctly_at_both` |
| TR-2 | `a_virtualized_conversation_paints_what_the_whole_one_did`, `a_window_covers_the_viewport_and_starts_inside_the_item_it_lands_in`, `a_conversation_nothing_has_measured_has_no_window_and_no_anchor`, `frame_work_is_bounded_by_the_viewport_and_not_by_the_history` |
| TR-3 | `an_anchor_round_trips_through_the_row_it_names`, `a_conversation_resized_away_and_back_paints_the_frame_it_had`, `an_anchor_survives_a_width_change_and_a_row_number_does_not`, `a_resized_conversation_keeps_the_reader_on_the_same_message`, `a_wheel_notch_moves_the_conversation_the_same_distance_with_an_inspector_open` |
| TR-4 | `a_followed_viewport_moves_with_its_content_and_a_parked_one_does_not`, `a_conversation_scrolled_back_to_the_end_keeps_up_and_a_parked_one_stays_put`, `scrolling_clamps_to_the_content_and_reports_a_boundary_as_no_movement` |
| TR-5 | `each_conversation_keeps_its_own_reading_position` |
