# Spec — Transcript layout

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | How tall a conversation is, which part of it a frame builds, and where its reader is |
| Depends on | [surface-model](./surface-model.md) SURF-5 for retained state; the wheel rules in [interaction-routing](./interaction-routing.md) INV-3 |
| Proven by | `plexmaton-tui::transcript`, `::render`, and `::state::scroll` tests; see the evidence table |

## Purpose

A conversation is the one surface whose content outgrows its viewport by orders of magnitude and
keeps changing while somebody reads it. Laid out like the other panels — build every line, wrap the
lot, show a slice — a frame costs the whole history, and the exit gate rules that out: large
transcripts must not require full-history rendering for a frame.

The second half of the problem is where the reader is. A row number answers that only until the
next resize, because how many rows precede a message depends on how wide the panel is.

## Invariants

**TR-1 — A height is measured once per item, revision, and width.** An item's height comes from the
same wrapper that paints it (D-039), and is recomputed only when that item's revision or the panel's
width changes. A streaming delta re-measures one item; a resize re-measures each item once; an
unchanged frame re-measures none.

The width is part of the *key*, not merely a validity check: a conversation changes width when the
terminal is resized or when the second window opens beside it at ultrawide, and it comes back to
the width it had. One set of heights per agent made each change invalidate the other width's, and
the counts above became one whole history per change. A conversation therefore keeps one set per
width, bounded at two, evicting the width least recently measured.

**TR-2 — A frame builds only what it draws.** The lines a frame constructs are bounded by the
viewport, not by the conversation's length. Off-screen items contribute their measured height and
nothing else.

**TR-3 — A reading position is an item, not a row.** A parked conversation is anchored to a
transcript item and a row inside it. The same text stays on screen across a resize, and content
arriving elsewhere in the conversation does not move it.

Turning a row into an item and back is width-dependent, so both directions resolve at the width that
produced the viewport being scrolled — carried out of the frame on the viewport itself, never read
from whichever width the cache measured last. Otherwise a wheel notch in one panel names an item
from the other panel's layout, and the reader lands on a message they never scrolled to.

**TR-4 — Following the tail is a state.** A viewport at its last row is *following* and stays at the
newest line as content arrives. Scrolling away parks it; scrolling back to the end resumes
following. Deriving this from `offset == max_offset` loses it at the first delta, which is the one
moment it matters. A conversation shorter than its viewport is painted at the bottom of it, so the
newest line is at the bottom whether or not the history overflows, and a window floating over the
top covers empty rows or rows already read (`ui-ux.md` §shelf).

**TR-5 — A reading position belongs to the conversation.** Each agent's transcript keeps its own
position, so selecting another agent and returning restores where its reader was (SURF-5).

## Model

```text
ViewState (immutable to the renderer)        TranscriptMetrics (outlives the frame)
   TranscriptPosition per agent  ────────▶   offset_of ──▶ row
                                             window    ──▶ items this frame builds
   wheel ◀──────────────────────────────────  anchor_at ◀── row
```

### Ownership

| Fact | Owner | Why not elsewhere |
| --- | --- | --- |
| Wrapped item heights | `TranscriptMetrics`, held by the [frame loop](./frame-loop.md), keyed by agent and width | The renderer takes the projection by shared reference, and a cache that dies with the frame is not one |
| The width a surface's rows were measured at | `Viewport::content_width`, filled in by the renderer | A row count means nothing without it, and the scroll path is not where the renderer's arithmetic should be repeated |
| Where each reader is | `state::scroll::ScrollState`, keyed by agent | It is user intent, and it has to survive frames and agent switches |
| Turning a row into an item and back | `TranscriptMetrics` | Both directions need the same heights; two implementations would disagree at exactly one width |
| What an item's lines are | `content::transcript_item` | Measuring and painting must be given identical input or the height is a guess |

### Anchors

An anchor is `{ item, rows }`. The item is the durable half. The row inside it is width-dependent
like every other row count, so resolving clamps it to that item's height at the current width —
without the clamp, a message that wrapped shorter would be overshot and the *next* message would
appear at the top.

An anchor whose item no longer exists resolves to the tail. Nothing removes an item in Phase 00;
this is the prepared answer for a future that trims history, where rejoining the live conversation
is the least surprising place to land.

### Cost

| Change | Items wrapped |
| --- | --- |
| A delta on one item | 1 |
| A new item | 1 |
| A resize | every item, once |
| Anything else, including scrolling | 0 |

Summing measured heights still visits every item each frame, as does checking that each cached
height is still valid. That is arithmetic over a `Vec`, not layout; the claim is about wrapping, and
`TranscriptMetrics::wrapped` and `::lines_built` are what make it testable rather than asserted.
What those walks cost, and the length at which they would start to matter, is measured in
[`frame-loop`](./frame-loop.md) §cost.

## Failure modes

| Situation | Response |
| --- | --- |
| Panel too narrow to wrap into | An item measures zero rows rather than dividing by a zero width |
| A conversation with no items | The panel falls back to a placeholder; there is nothing to virtualize |
| A conversation nothing has measured | No window and no anchor, so the wheel leaves it alone rather than parking it at a guess |
| An offset past the end of the content | An empty window. Drawing something arbitrary would hide the mis-clamp that produced it |
| Content taller than `u16::MAX` rows | Saturates. A viewport offset is a `u16` because that is what the terminal addresses |
| An anchored item that no longer exists | Resolves to the tail |

## Out of scope

- **Bounded overscan**, which the phase's scope names. A frame here is synchronous and exact:
  building `n` extra items costs `n` extra wraps and prevents nothing, because there is no
  asynchronous fill for it to hide. It arrives with a renderer that can be behind.
- **Expand and collapse for tool activity and artifacts.** Their surface is the inspector
  (delivery step 7).
- **Retention limits and cache pruning.** Nothing drops a transcript item in Phase 00, so the cache
  is bounded by the projection it mirrors. Pruning arrives with whatever first drops one.
- **Which surface the wheel reaches.** [`interaction-routing`](./interaction-routing.md) INV-3.

## Evidence

| Invariant | Proven by |
| --- | --- |
| TR-1 | `measurement_is_proportional_to_what_changed`, `item_heights_sum_to_the_height_of_the_whole_conversation`, `the_resize_workload_re_measures_every_item_exactly_once`, `two_widths_of_one_conversation_do_not_invalidate_each_other`, `a_run_of_widths_retains_only_the_last_two`, `a_conversation_drawn_at_two_widths_measures_correctly_at_both` |
| TR-2 | `a_virtualized_conversation_paints_what_the_whole_one_did`, `a_window_covers_the_viewport_and_starts_inside_the_item_it_lands_in`, `a_conversation_nothing_has_measured_has_no_window_and_no_anchor`, `frame_work_is_bounded_by_the_viewport_and_not_by_the_history` |
| TR-3 | `an_anchor_round_trips_through_the_row_it_names`, `an_anchor_survives_a_width_change_and_a_row_number_does_not`, `a_resized_conversation_keeps_the_reader_on_the_same_message`, `a_wheel_notch_moves_the_conversation_the_same_distance_with_an_inspector_open` |
| TR-4 | `a_followed_viewport_moves_with_its_content_and_a_parked_one_does_not`, `a_conversation_scrolled_back_to_the_end_keeps_up_and_a_parked_one_stays_put`, `scrolling_clamps_to_the_content_and_reports_a_boundary_as_no_movement` |
| TR-5 | `each_conversation_keeps_its_own_reading_position` |
