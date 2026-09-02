# Spec — Frame loop

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | When a frame is drawn, what work it may cost, and what the next event resolves against |
| Depends on | [transcript-layout](./transcript-layout.md) TR-1 and TR-2 for what a conversation costs; [surface-model](./surface-model.md) SURF-1 for what a frame registers |
| Proven by | `plexmaton-tui::workspace` and `plexmaton-cli::measure` tests; see the evidence table |

## Purpose

A terminal workspace has no compositor telling it when to repaint, so it decides for itself. Wrong
in one direction and the screen redraws on every idle tick of ambient background traffic; wrong in
the other and a resize leaves the previous frame describing a terminal that no longer exists.

The second half is measurement. *Responsive* is not a property a document can assert, and a budget
recorded without a harness is a guess. So the loop is one object rather than a shape each caller
assembles, and what a frame cost is a value it returns.

## Invariants

**FR-1 — A frame is drawn only when something changed.** The projection's revision gates the
repaint. A terminal resize changes what a frame means without changing the projection, so it
invalidates the last frame explicitly rather than being inferred. Producer traffic that alters
nothing visible costs no frame at all.

**FR-2 — A frame's layout work is bounded by its viewport, not by the conversation's length.** What
a frame wraps and builds is what its viewport reaches (TR-2). Two frames are deliberate exceptions
and cost one wrap per item: the first frame on a conversation, and the first frame at a new width.
Both are what every cheap frame afterwards is paying for.

**FR-3 — An event resolves against the frame that was drawn.** Hit testing, wheel targeting, and
scroll anchoring all read the registry and the item heights the last frame produced. Before the
first frame nothing is registered, so a pointer event resolves to nothing rather than to a guessed
region (SURF-1).

## Model

```text
producer events ─▶ Workspace::emit ─┐
                                    ├─▶ ViewState ──▶ draw ──▶ Option<FrameWork>
terminal events ─▶ Workspace::handle┘        │                    │
                          │                  │                    └─▶ SurfaceTree + heights
                          └── Outcome ───────┴──── the next handle reads them (FR-3)
```

### What the workspace holds, and why together

| Field | Why it cannot live elsewhere |
| --- | --- |
| `ViewState` | The one projection. Everything below is derived from it or resolved against it |
| `Router` | Owns pointer capture, which spans events and so cannot be rebuilt per event |
| `SurfaceTree` | The registry the last frame drew, which is what the next event hit-tests (FR-3) |
| `TranscriptMetrics` | Wrapped heights, which outlive the frame that measured them (TR-1) |
| `painted` | The revision the screen is showing, which is the whole of FR-1 |

The executable adds the two things only a process has: a real terminal, and an asynchronous wait on
producer and terminal events at once. The measurement harness substitutes a cell buffer for the
first and a scripted timeline for the second, and drives the same object through the same methods —
so a measured frame is the frame the user gets, rather than a second loop nobody runs.

### Cost, measured

Layout work per frame is flat in the conversation's length; total frame cost is not. A frame still
walks the item list twice in full — once to check every cached height is still valid, once to sum
them — and three times partially, to resolve the reader's anchor, locate the window, and reach the
first item it builds. That is pointer arithmetic rather than wrapping, and it is why FR-2 is stated
about layout work specifically.

The distinction is not academic: it is the difference between a frame that stays under a
millisecond at five thousand messages and one that would not. Linear in cheap operations, the
walks become the budget somewhere around fifty thousand messages, which is where to look first and
not before.

Observed with `cargo run --release -p plexmaton-cli --bin plexmaton-measure` on an `arm64` macOS
machine, release profile, 120 × 40, over a 5,000-message conversation. The targets are the
contract's ([`ui-ux.md`](../roadmap/ui-ux.md) §performance budgets).

| Workload | Observed | Work |
| --- | --- | --- |
| `streaming delta` | 0.9 ms p50, 1.2 ms max | 1 item wrapped, 27 lines built, at any history length |
| `wheel` | 0.9 ms p50, 1.0 ms max | 0 wrapped |
| `open inspector` | 1.3 ms p50, 1.5 ms max | 0 wrapped |
| `two conversations` | 1.4 ms p50, 1.5 ms max | 0 wrapped |
| `extend selection` | 1.2 ms p50, 1.5 ms max | 0 wrapped |
| `cold open`, `open hidden conversation` | 12.6 to 12.9 ms p50, 13.4 ms max | 5,000 wrapped, once |
| `resize` | 12.4 ms p50, 14.3 ms max | 5,000 wrapped, once per width |
| retained | 10,000 entries for two 5,000-message conversations; at most two widths per conversation (TR-1) | |

Read the timings as an order of magnitude. The same binary on the same laptop under compile load
measured roughly double every row, and re-running the previous commit under that load reproduced
the loaded numbers, so the spread is the machine and not the code. A third machine under its own
load reported 25.9 ms and 29.1 ms for the two cold rows. The three rows that scale with history
are one cost paid three times, measuring every item once; when something needs it, the fix is a
retention limit or a lazily measured tail, not a faster wrap. Opening the window and selecting
measure nothing, because a shelf keeps the conversation's width and a selection changes a style,
never a character; only a change of width invalidates a height.

### Work is asserted; time is reported

Two kinds of number, and conflating them makes a performance lane either noise or theatre. Items
wrapped, lines built, frames painted, and cache entries retained are identical on every machine, so
they are ordinary test assertions and a regression in them fails a build. Wall-clock latency belongs
to whichever machine ran the command, so it is recorded beside that machine and its build profile
and is never asserted. Rejected: asserting wall-clock budgets, a flaky test wearing a budget's
clothes; and `criterion`, which cannot see work counts.

## Failure modes

| Situation | Response |
| --- | --- |
| Nothing changed since the last frame | `draw` returns `None`. A caller that cannot tell this from a frame cannot measure how often the gate fires |
| A terminal below the supported minimum | One notice, and no surfaces registered, so the next event resolves to nothing |
| An event arriving before the first frame | Routed against an empty registry and declined by name, never against a layout computed on the side |
| A backend that fails to draw | The backend's own error is returned; the loop neither swallows it nor keeps a stale `painted` |
| A submitted draft with no agent to receive it | Stays in the draft. The workspace hands text back as a value and never delivers it itself (COM-3) |
| A workload that painted no frames | Its percentiles are zero rather than a division by no samples |

## Out of scope

- **Which surfaces exist, and what each one draws.** Layout and the renderer;
  [`surface-model`](./surface-model.md).
- **Which surface an event reaches.** [`interaction-routing`](./interaction-routing.md).
- **The targets.** [`ui-ux.md`](../roadmap/ui-ux.md) §performance budgets owns them; this file
  owns what was observed against them.
- **Continuous performance regression checking.** Numbers from a shared runner would set budgets
  nobody can reproduce. The command is the citation and the machine is named beside the number.

## Evidence

| Invariant | Proven by |
| --- | --- |
| FR-1 | `a_frame_is_drawn_only_when_something_changed`, `ctrl_c_quits_from_anywhere` |
| FR-2 | `frame_work_is_bounded_by_the_viewport_and_not_by_the_history`, `scrolling_a_measured_conversation_wraps_nothing`, `the_wheel_workload_costs_no_measurement`, `the_resize_workload_re_measures_every_item_exactly_once`, `a_background_agent_streaming_does_not_re_measure_the_foreground` |
| FR-3 | `the_wheel_moves_a_drawn_viewport_and_nothing_before_one_exists`, `tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in`, `typing_reaches_the_composer_and_submitting_hands_the_text_back` |
