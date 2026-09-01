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
repaint (D-004). A terminal resize changes what a frame means without changing the projection, so it
invalidates the last frame explicitly rather than being inferred. Producer traffic that alters
nothing visible costs no frame at all.

**FR-2 — A frame's layout work is bounded by its viewport, not by the conversation's length.** What
a frame wraps and builds is what its viewport reaches (TR-2). Two frames are deliberate exceptions
and cost one wrap per item: the first frame on a conversation, and the first frame at a new width.
Both are what every cheap frame afterwards is paying for.

**FR-3 — An event resolves against the frame that was drawn.** Hit testing, wheel targeting, and
scroll anchoring all read the registry and the item heights the last frame produced. Before the
first frame nothing is registered, so a pointer event resolves to nothing rather than to a guessed
region (D-036).

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
millisecond at five thousand messages and one that would not. The numbers, and the scale at which
the walks would start to matter, are in [`ui-ux.md`](../roadmap/ui-ux.md) §UX performance budgets.

### Work is asserted; time is reported

Two kinds of number, and conflating them makes a performance lane either noise or theatre. Items
wrapped, lines built, frames painted, and cache entries retained are identical on every machine, so
they are ordinary test assertions and a regression in them fails a build. Wall-clock latency belongs
to whichever machine ran the command, so it is recorded beside that machine and its build profile
and is never asserted — a timing assertion on a shared machine is a flaky test wearing a budget's
clothes.

## Failure modes

| Situation | Response |
| --- | --- |
| Nothing changed since the last frame | `draw` returns `None`. A caller that cannot tell this from a frame cannot measure how often the gate fires |
| A terminal below the supported minimum | One notice, and no surfaces registered, so the next event resolves to nothing (D-025) |
| An event arriving before the first frame | Routed against an empty registry and declined by name, never against a layout computed on the side |
| A backend that fails to draw | The backend's own error is returned; the loop neither swallows it nor keeps a stale `painted` |
| A submitted draft with no agent to receive it | Stays in the draft. The workspace hands text back as a value and never delivers it itself (COM-3) |
| A workload that painted no frames | Its percentiles are zero rather than a division by no samples |

## Out of scope

- **Which surfaces exist, and what each one draws.** Layout and the renderer;
  [`surface-model`](./surface-model.md).
- **Which surface an event reaches.** [`interaction-routing`](./interaction-routing.md).
- **The budget numbers themselves.** [`ui-ux.md`](../roadmap/ui-ux.md) §UX performance budgets, so
  that a target and the mechanism that meets it do not drift apart in two files.
- **Continuous performance regression checking.** Numbers from a shared runner would set budgets
  nobody can reproduce. The command is the citation and the machine is named beside the number.

## Evidence

| Invariant | Proven by |
| --- | --- |
| FR-1 | `a_frame_is_drawn_only_when_something_changed`, `ctrl_c_quits_from_anywhere` |
| FR-2 | `frame_work_is_bounded_by_the_viewport_and_not_by_the_history`, `scrolling_a_measured_conversation_wraps_nothing`, `the_wheel_workload_costs_no_measurement`, `the_resize_workload_re_measures_every_item_exactly_once`, `a_background_agent_streaming_does_not_re_measure_the_foreground` |
| FR-3 | `the_wheel_moves_a_drawn_viewport_and_nothing_before_one_exists`, `tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in`, `typing_reaches_the_composer_and_submitting_hands_the_text_back` |
