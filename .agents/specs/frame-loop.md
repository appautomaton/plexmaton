# Spec — Frame loop

| Field | Value |
| --- | --- |
| Status | Implemented; saturated real-terminal stream latency remains unmeasured |
| Owns | When a frame is drawn, what work it may cost, what the next event resolves against, and how cost is measured |
| Depends on | [transcript-layout](./transcript-layout.md) TR-1 and TR-2 for what a conversation costs; [surface-model](./surface-model.md) SURF-1 for what a frame registers; the targets in [`ui-ux.md`](../ui-ux.md) §performance budgets |
| Proven by | `plexmaton-tui::workspace`, `plexmaton-cli::stream_frames` and `::measure` tests |

## Invariants

**FR-1 — A frame is drawn only when something changed.** The projection's revision gates the
repaint. A terminal resize or changed palette alters what a frame means without changing the projection,
so it invalidates the last frame explicitly; an identical palette assignment is a no-op. The quit chord's owned one-shot deadline advances the
projection once when its visible question expires. A captured edge drag owns a 60 ms deadline only
while it can move; each wake changes its viewport or disarms. Neither is an ambient animation
clock. Producer traffic that alters nothing visible costs no frame. An admitted preparation
completion invalidates presentation without changing semantic state (PRE-3).
An owned status command may publish a changed footer (STL-4); only changed decoded output advances
the view revision. Its optional refresh clock schedules a command, not a frame or transcript scan.

**FR-2 — A frame's layout work is bounded by its viewport, not by the conversation's length.** What
a frame builds is what its viewport reaches (TR-2). Cold/new-width literal heights use count-only
wrapping; rich/disclosed entries use provisional heights and request only reached preparation
(PRE-3), never a full-history parser pass.

**FR-3 — An event resolves against the frame that was drawn.** Hit testing, wheel targeting, and
scroll anchoring read the registry and the heights the last frame produced. Before the first frame
nothing is registered, so a pointer event resolves to nothing rather than a guessed region (SURF-1).
Prepared text maps are pinned to the successful frame; cache admission cannot replace them beneath input.
MTH-5 commits native reservations and those maps only after both cell and native output succeed;
the native scene participates in the same resize, clipping and overlay ownership.

**FR-4 — Work is asserted; time is reported.** Entries wrapped, lines built, frames painted, and
cache entries retained are identical on every machine, so they are test assertions and a regression
fails a build. Wall-clock latency belongs to the machine that ran the command and is recorded beside
it, never asserted. A workload that promises one changed frame per sample asserts that exact count;
a boundary no-op cannot silently shorten the run. Rejected: asserting wall-clock budgets, a flaky test wearing a budget's clothes;
`criterion`, which cannot see work counts; and a shared runner's numbers as budgets, which nobody
can reproduce.

**FR-5 — Stream frames coalesce without moving unseen text beneath input.** Pending transcript
deltas enter the projection in order before their frame or after an input target has resolved
against the painted projection, never by rewriting or discarding source. A bounded batch owns one
non-sliding deadline; non-text transitions, input, pressure and completion flush it early, and an
empty batch owns no wake. Rejected: rebuilding a newer Markdown map during hit testing of old
screen coordinates; PRE-3 removes that implicit parser path.

## Model

```text
producer events ─▶ Workspace::emit ─┐
                                    ├─▶ ViewState
terminal events ─▶ Workspace::handle┘        │
                          │                  ▼
                          │          Workspace::draw
                          │            ├─ selection/copy preflight
                          │            ├─ render(&ViewState): candidate frame
                          │            └─ cell/native success: publish frame maps
                          └── Outcome          └─ next input resolves against them (FR-3)
```

The workspace's preflight can invalidate obsolete UI selection/copy state before rendering;
`render` remains a projection borrowed through `&ViewState`. Only successful cell/native output
publishes the new surface registry, native scene and source pins. Output failure keeps those
published maps and does not roll back a preflight invalidation.

The workspace is one object: the projection, the router with its capture, the registry the last
frame drew, the wrapped heights, and the revision the screen shows. The executable adds a real
terminal and an asynchronous wait on producer and terminal events at once. `StreamFrames` owns
only pending `TranscriptDelta` envelopes, not an authoritative transcript; reports and session
replacement flush preceding deltas first. Its limits are:

| Boundary | Policy |
| --- | --- |
| Background frame interval | 16 ms from the last successful frame's start; later deltas do not extend it |
| Pending events | At most 64; reaching the limit flushes early |
| Pending text allocation | At most 128 KiB of `String` capacity; pressure flushes early, and an individually oversized event is applied without retaining it in the batch |
| Input | Resolve the original event first, then flush; input frames are not rate-limited |
| Idle / failed output | Empty batches have no deadline; a failed draw advances neither the successful frame count nor its deadline |

An unchanged projection still costs no frame, and the next loop iteration checks a due frame even
when a different ready event won the asynchronous wait. This is coalescing, not a hard global FPS
cap: pressure and interaction can paint sooner. The measurement harness substitutes a cell buffer
and explicit time; its stream workload drives the same batching, input and drawing methods.

### Cost, measured

`cargo run --release -p plexmaton-cli --bin plexmaton-measure` reports three distinct instruments:
CPU-only reference samples explicitly prepare through the public worker seam outside `draw`;
persistent-process batches include pipes and decoding; live preparation samples include actual
process-to-workspace adoption and every cell-buffer frame until reached data settles. None includes
terminal transport or waiting for the outer input loop. Work counts are asserted, timings are not.

A CPU sample may include a retained-content frame and a prepared frame; a cold or evicted entry
can instead show a pending placeholder. Frame/preparation counts alone do not measure blanking.
Its wrapped/line counts are
totals for the sample, not one physical frame, and cannot be compared directly with old synchronous
single-frame timings. Palette, hover and retained selection still repaint without preparation.
Visible conversations retain full entry walks for height validation, title counts and anchor
resolution; request admission also locates its bounded set of source identities in that order.

The timing tables below are the baseline at `448b013`, before retained streaming presentation;
current release timings remain unmeasured. Observed on arm64 macOS, release, 2026-09-06,
120 × 40, with a 5,000-message base history and 625
interleaved tools where the workload uses messages; repeated workloads use 200 samples, cold uses
ten. New event workloads grow their history during the run:

| CPU reference sample | p50 | Height work |
| --- | --- | --- |
| Streaming delta | 6.24 ms | One changed entry; two frames total |
| Compact tool transition | 7.02 ms | One changed entry; two frames total |
| Tool disclosure | 2.49 ms | One changed entry |
| Wheel | 2.47 ms | No repeated height measurement; reached cache misses may add preparation |
| Palette repaint | 1.85 ms | No heights or prepared rows rebuilt |
| Inspector open / two conversations | 3.52 / 4.73 ms | Retained heights reused |
| Entry selection / text drag | 2.12 / 2.41 ms | No repeated heights or text maps |
| Cold open / hidden conversation | 21.66 / 17.89 ms | Literal heights once; reached preparation settles |
| Resize | 18.50 ms; p95 23.45 ms | New-width literal heights once |
| Retained heights | 11,250 for two 5,625-entry conversations | At most two widths per conversation |

These settled CPU samples do not establish the first-input-frame budget. Cold plain history and
resize still exceed targets in some samples; moving preparation off-loop is not a claim that every
remaining scan or terminal write is cheap.

### Rich history and live preparation

Every entry contains a heading, inline code/emphasis/link, quote and list. CPU-reference cold
120 × 40 and new-width 88 × 40 samples take medians of three, followed by a palette-only repaint:

| Entries | Cold / resize CPU sample | Palette repaint | Prepared entries |
| --- | --- | --- | --- |
| 500 | 1.19 / 1.27 ms | 0.22 ms | 16 cold/new width; zero on repaint |
| 5,000 | 13.63 / 14.34 ms | 1.73 ms | 16 cold/new width; zero on repaint |

The real executable, framed pipes, workspace admission and cell-buffer painting use the same rich
source. At 5,000 entries, median of three cold samples:

| Width × height | First pending paint | Reached data settled | Frames / prepared entries |
| --- | --- | --- | --- |
| 120 × 40 | 1.18 ms | 15.78 ms | 2 / 16 |
| 88 × 40 | 1.25 ms | 16.56 ms | 2 / 16 |
| 60 × 40 | 1.20 ms | 16.24 ms | 2 / 16 |

The preceding single-entry live adapter took nineteen frames and 41.6–42.4 ms for the same history
and widths. Bounded batching removes those repeated paints; it does not prove physical terminal
latency. The independent persistent-child workload runs 32 batches of sixteen rich entries:
2.54 ms for its first batch and 0.209 ms warm median, including transport and decoding.

### Coalesced rich streaming

The reference driver appends 160 deltas to one existing rich entry over warm 500/5,000-message
histories. Its prefix repeats a heading, styled/link/code paragraph, quote and list 32 times; deltas
include incomplete delimiters and Unicode, with one typed character halfway through. Exact source,
revision and retained input are checked. Both drawing strategies explicitly settle preparation
in-process, so this compares scheduling/CPU work, not real-process input latency.

| 5,626-entry CPU workload | Frames | Prepared entries | Processing median | Typed-input sample |
| --- | --- | --- | --- | --- |
| Burst, immediate reference | 321 | 160 | 763.8 ms | 1.89 ms |
| Burst, production coalescer | 8 | 4 | 19.0 ms | 4.75 ms |
| 1 ms arrivals, immediate reference | 321 | 160 | 759.6 ms | 1.89 ms |
| 1 ms arrivals, production coalescer | 21 | 10 | 49.4 ms | 1.85 ms |

The smaller history has identical frame/preparation counts. Arrival time is explicit;
processing time excludes scheduled waiting. PRE-2 and SEL-8 separately prove real production-loop
input, overlays and resize progress while preparation or clipboard helpers are blocked, including
reaping on exit. Saturated real-terminal streaming and native-math output remain unmeasured.

## Failure modes

| Situation | Response |
| --- | --- |
| Nothing changed since the last frame | `draw` returns `None`, so a caller can count how often the gate fires |
| A terminal below the supported minimum | One notice, and no surfaces registered, so the next event resolves to nothing |
| An event arriving before the first frame | Routed against an empty registry and declined by name |
| A backend that fails to draw | The backend's own error is returned; the loop neither swallows it nor keeps a stale `painted` |
| A submitted draft with no agent to receive it | Stays in the draft; the workspace hands text back as a value and never delivers it (COM-3) |
| A workload that painted no frames | Its percentiles are zero rather than a division by no samples |

## Evidence

| Invariant | Proven by |
| --- | --- |
| FR-1 | `palette_changes_reuse_heights_and_preserve_pointer_copy_at_three_widths`, `a_frame_is_drawn_only_when_something_changed`, `current_work_does_not_move_input_and_repeated_facts_cost_no_frame`, `the_quit_deadline_expires_once_and_costs_one_frame`, `ctrl_c_clears_a_draft_or_interrupts_but_never_does_both`, `an_edge_drag_scrolls_and_copies_entries_that_started_off_screen` |
| FR-2 | `frame_work_is_bounded_by_the_viewport_and_not_by_the_history`, `scrolling_a_measured_conversation_wraps_nothing`, `the_wheel_workload_costs_no_measurement`, `opening_and_closing_the_inspector_records_every_sample`, `compact_tool_entries_cost_one_wrap_at_any_history_length`, `opening_a_tool_entry_costs_one_wrap_and_not_its_history`, `the_resize_workload_re_measures_every_entry_exactly_once`, `a_background_agent_streaming_does_not_re_measure_the_foreground`, `a_native_tool_round_trip_is_a_stream_the_projection_accepts` |
| FR-3 | `pending_stream_copy_captures_painted_fragments_without_waiting_for_new_source`, `ready_preparation_keeps_pointer_copy_on_the_painted_source_until_the_next_frame`, `failed_native_output_keeps_the_last_painted_hit_map_and_frame_identity`, `native_runs_keep_their_origin_and_never_cross_viewport_or_overlay_edges`, `prepared_text_is_not_selectable_until_the_result_has_been_painted`, `blocked_preparation_never_holds_the_production_input_and_frame_loop`, `the_wheel_moves_a_drawn_viewport_and_nothing_before_one_exists`, `tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in`, `typing_reaches_the_composer_and_submitting_hands_the_text_back` |
| FR-4 | `preparation_result_count_includes_results_dropped_by_retention`, `palette_workload_repaints_without_height_work_at_both_history_scales`, `rich_history_measurement_separates_cold_resize_and_paint_work`; The FR-2 rows assert work counts; `extending_selection_records_every_declared_sample` pins sample accounting; `plexmaton-measure` prints time and asserts none of it |
| FR-5 | `a_prepared_revision_overtaken_by_queued_deltas_is_still_painted_and_copyable`, `stream_deadline_is_fixed_and_idle_owns_no_wake`, `stream_event_pressure_bounds_batches_and_finalization_flushes_the_tail`, `stream_byte_pressure_counts_capacity_and_does_not_drop_oversized_events`, `input_and_interrupt_flush_streams_without_waiting_for_the_frame_interval`, `stream_copy_uses_the_painted_markdown_before_applying_queued_delimiters`, `resize_flushes_pending_text_and_replaces_geometry_only_after_drawing`, `explicit_flush_retains_the_final_partial_stream_without_another_arrival`, `failed_stream_frame_does_not_acknowledge_paint_or_replay_its_deltas`, `rich_stream_measurement_asserts_frame_work_and_exact_source_at_both_scales`; applying deltas before mouse release was mutation-tested and failed the copy witness |
