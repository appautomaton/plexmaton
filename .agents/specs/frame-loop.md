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
clock. Producer traffic that alters nothing visible costs no frame.
An owned status command may publish a changed footer (STL-4); only changed decoded output advances
the view revision. Its optional refresh clock schedules a command, not a frame or transcript scan.

**FR-2 — A frame's layout work is bounded by its viewport, not by the conversation's length.** What
a frame wraps and builds is what its viewport reaches (TR-2). Two frames are the exceptions and cost
one wrap per entry: the first frame on a conversation, and the first frame at a new width.

**FR-3 — An event resolves against the frame that was drawn.** Hit testing, wheel targeting, and
scroll anchoring read the registry and the heights the last frame produced. Before the first frame
nothing is registered, so a pointer event resolves to nothing rather than a guessed region (SURF-1).

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
empty batch owns no wake. Rejected: advancing text projection but delaying its frame, which lets
hit testing rebuild a new Markdown map for old screen coordinates.

## Model

```text
producer events ─▶ Workspace::emit ─┐
                                    ├─▶ ViewState ──▶ draw ──▶ Option<FrameWork>
terminal events ─▶ Workspace::handle┘        │                    │
                          │                  │                    └─▶ SurfaceTree + heights
                          └── Outcome ───────┴──── the next handle reads them (FR-3)
```

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

Layout work per frame is flat in the conversation's length; total frame cost is not. Each visible
conversation currently takes one full semantic-entry pass to validate cached heights and another
to derive the counts in its title. Each sub-agent row takes one count pass, and an inspected
conversation's title takes its own. The cached heights are walked once in full to sum rows, then
partially to resolve the anchor and window; building reaches the visible entries through the
semantic iterator. The current-work label adds one primary-entry
scan. These passes are arithmetic, not wrapping, and become the budget somewhere around fifty
thousand entries, which is where to look first and not before.

Observed with `cargo run --release -p plexmaton-cli --bin plexmaton-measure` on an `arm64` macOS
machine on 2026-09-05, release profile, 120 × 40, over 5,000-entry histories; the message history
has 625 additional interleaved tool entries. This first table measures individual frames over
predominantly plain text, not the batch scheduler or a rich-Markdown history budget:

| Workload | Observed | Work |
| --- | --- | --- |
| `streaming delta` | 1.5 ms p50, 1.9 ms max | 1 entry wrapped, 37 lines built, at any history length |
| `compact tool entry` | 1.4 ms p50, 1.5 ms max | 1 entry wrapped, 35 lines built, at any history length |
| `open tool entry` | 1.7 ms p50, 1.8 ms max | 1 entry wrapped, 39 lines built, at any history length |
| `wheel` | 1.4 ms p50, 1.5 ms max | 0 wrapped |
| `open inspector` | 2.7 ms p50, 3.0 ms max | 0 wrapped |
| `two conversations` | 2.8 ms p50, 3.1 ms max | 0 wrapped |
| `extend selection` | 1.7 ms p50, 1.9 ms max | 0 wrapped |
| `text drag` | 1.7 ms p50, 1.8 ms max | 0 wrapped, 0 map rebuilds; every sample paints |
| `cold open`, `open hidden conversation` | 6.3 / 8.9 ms p50, 9.2 ms max | 5,625 heights, once; literal messages count shared breaks without preparing hidden layouts |
| `resize` | 7.1 ms p50, 7.9 ms p95, 8.2 ms max | 5,625 heights, once per width |
| `palette change` | 1.5 ms p50, 1.6 ms max | 0 heights remeasured; every sample repaints |
| retained | 11,250 entries for two 5,625-entry conversations; at most two widths per conversation (TR-1) | |

Read the timings as machine-local observations, not latency guarantees. The predominantly plain-text cold/new-width workload now fits the declared budgets. The three rows that
scale with history measure every entry once. A retained-layout hit avoids Markdown parsing, but
does not remove those cold passes. Opening an already measured window and selecting require no
height measurement; a new width or changed entry invalidates the affected heights. Palette replacement rebuilds only
reached styled layouts, without discarding height geometry or selection. The paired pre-change run
on this worktree measured 25.9 ms cold open and 26.0 ms resize; work counts and visible output are
unchanged by the shared-break/count-only path.

### Coalesced rich streaming, measured

The same command compares 160 deltas appended to one existing rich-Markdown entry over warm
500/5,000-message histories. The prefix repeats a heading, styled/link/code paragraph, quote and
list 32 times; deltas include incomplete delimiters and Unicode. One typed character arrives
halfway through. The immediate reference is the former per-event `Workspace::draw` path, not a
second production implementation. Each run validates exact source, final revision and retained
input; time is the median of three repetitions at 120 × 40.

| 5,626-entry workload | Frames | Layout/map builds | Processing time | Typed-input frame |
| --- | --- | --- | --- | --- |
| Burst, immediate reference | 161 | 160 | 259.0 ms | 1.45 ms |
| Burst, production coalescer | 4 | 4 | 6.5 ms | 1.60 ms |
| 1 ms arrivals, immediate reference | 161 | 160 | 258.1 ms | 1.41 ms |
| 1 ms arrivals, production coalescer | 11 | 10 | 17.6 ms | 1.46 ms |

The smaller history has the same frame/layout counts. Arrival time is explicit and deterministic;
processing time excludes scheduled waiting and real terminal output, and the typed-input sample
excludes waiting for the outer event loop. These figures do not establish end-to-end input latency
under saturated provider traffic, animation or slow terminal I/O. Cold rich-text preparation, paint-only reuse of the visible styled layouts and awaited clipboard
helpers remain separate work.

### Cold rich histories, measured

The same harness also measures 500/5,000 assistant entries, each with a short heading, inline
code/emphasis/link, quote and list. The median of three runs measures cold 120 × 40, new-width
88 × 40, then a palette-only repaint. Input and terminal I/O are excluded.

| Entries | Cold / resize | Palette repaint | Work |
| --- | --- | --- | --- |
| 500 | 3.0 / 3.1 ms | 0.2 ms | 500 layouts at cold/new width; 0 heights and 4 visible layouts on repaint |
| 5,000 | 30.3 / 30.5 ms | 1.1 ms | 5,000 layouts at cold/new width; 0 heights and 4 visible layouts on repaint |

This richer cold path still exceeds budget. Preparing it off the interaction loop requires owned
jobs, bounded revision/cache handling and safe adoption against the painted source; it is not
fixed by the faster literal path. Palette-driven animation must also stop reparsing visible rich
entries. No end-to-end responsiveness or animation guarantee follows from these CPU samples.

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
| FR-3 | `the_wheel_moves_a_drawn_viewport_and_nothing_before_one_exists`, `tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in`, `typing_reaches_the_composer_and_submitting_hands_the_text_back` |
| FR-4 | `palette_workload_repaints_without_height_work_at_both_history_scales`, `rich_history_measurement_separates_cold_resize_and_paint_work`; The FR-2 rows assert work counts; `extending_selection_records_every_declared_sample` pins sample accounting; `plexmaton-measure` prints time and asserts none of it |
| FR-5 | `stream_deadline_is_fixed_and_idle_owns_no_wake`, `stream_event_pressure_bounds_batches_and_finalization_flushes_the_tail`, `stream_byte_pressure_counts_capacity_and_does_not_drop_oversized_events`, `input_and_interrupt_flush_streams_without_waiting_for_the_frame_interval`, `stream_copy_uses_the_painted_markdown_before_applying_queued_delimiters`, `resize_flushes_pending_text_and_replaces_geometry_only_after_drawing`, `explicit_flush_retains_the_final_partial_stream_without_another_arrival`, `failed_stream_frame_does_not_acknowledge_paint_or_replay_its_deltas`, `rich_stream_measurement_asserts_frame_work_and_exact_source_at_both_scales`; applying deltas before mouse release was mutation-tested and failed the copy witness |
