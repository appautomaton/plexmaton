# Spec — Frame loop

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | When a frame is drawn, what work it may cost, what the next event resolves against, and how cost is measured |
| Depends on | [transcript-layout](./transcript-layout.md) TR-1 and TR-2 for what a conversation costs; [surface-model](./surface-model.md) SURF-1 for what a frame registers; the targets in [`ui-ux.md`](../ui-ux.md) §performance budgets |
| Proven by | `plexmaton-tui::workspace` and `plexmaton-cli::measure` tests |

## Invariants

**FR-1 — A frame is drawn only when something changed.** The projection's revision gates the
repaint. A terminal resize changes what a frame means without changing the projection, so it
invalidates the last frame explicitly. The quit chord's owned one-shot deadline advances the
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
terminal and an asynchronous wait on producer and terminal events at once; the measurement harness
substitutes a cell buffer and a scripted timeline and drives the same object through the same
methods, so a measured frame is the frame the user gets.

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
has 625 additional interleaved tool entries. This corpus is predominantly plain text; these timings
do not establish a rich-Markdown history budget:

| Workload | Observed | Work |
| --- | --- | --- |
| `streaming delta` | 1.4 ms p50, 1.8 ms max | 1 entry wrapped, 37 lines built, at any history length |
| `compact tool entry` | 1.4 ms p50, 1.5 ms max | 1 entry wrapped, 35 lines built, at any history length |
| `open tool entry` | 1.6 ms p50, 1.7 ms max | 1 entry wrapped, 39 lines built, at any history length |
| `wheel` | 1.4 ms p50, 1.5 ms max | 0 wrapped |
| `open inspector` | 2.7 ms p50, 2.8 ms max | 0 wrapped |
| `two conversations` | 2.7 ms p50, 3.2 ms max | 0 wrapped |
| `extend selection` | 1.7 ms p50, 1.9 ms max | 0 wrapped |
| `text drag` | 1.7 ms p50, 1.8 ms max | 0 wrapped, 0 map rebuilds; every sample paints |
| `cold open`, `open hidden conversation` | 25.4 to 27.2 ms p50, 27.3 ms max | 5,625 wrapped, once |
| `resize` | 25.5 ms p50, 26.1 ms p95, 26.8 ms max | 5,625 wrapped, once per width |
| retained | 11,250 entries for two 5,625-entry conversations; at most two widths per conversation (TR-1) | |

Read the timings as machine-local observations, not latency guarantees. Cold open and new-width
layout exceed the 16 ms frame target. The three rows that
scale with history measure every entry once. A retained-layout hit avoids Markdown parsing, but
does not remove those cold passes. Opening an already measured window and selecting require no
height measurement; a new width, changed entry or palette invalidates the affected heights.

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
| FR-1 | `a_frame_is_drawn_only_when_something_changed`, `current_work_does_not_move_input_and_repeated_facts_cost_no_frame`, `the_quit_deadline_expires_once_and_costs_one_frame`, `ctrl_c_clears_a_draft_or_interrupts_but_never_does_both`, `an_edge_drag_scrolls_and_copies_entries_that_started_off_screen` |
| FR-2 | `frame_work_is_bounded_by_the_viewport_and_not_by_the_history`, `scrolling_a_measured_conversation_wraps_nothing`, `the_wheel_workload_costs_no_measurement`, `opening_and_closing_the_inspector_records_every_sample`, `compact_tool_entries_cost_one_wrap_at_any_history_length`, `opening_a_tool_entry_costs_one_wrap_and_not_its_history`, `the_resize_workload_re_measures_every_entry_exactly_once`, `a_background_agent_streaming_does_not_re_measure_the_foreground`, `a_native_tool_round_trip_is_a_stream_the_projection_accepts` |
| FR-3 | `the_wheel_moves_a_drawn_viewport_and_nothing_before_one_exists`, `tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in`, `typing_reaches_the_composer_and_submitting_hands_the_text_back` |
| FR-4 | The FR-2 rows assert work counts; `extending_selection_records_every_declared_sample` pins sample accounting; `plexmaton-measure` prints time and asserts none of it |
