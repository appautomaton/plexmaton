# Phase 00 — Experience Skeleton

| Field | Value |
| --- | --- |
| Status | In progress — steps 1 to 7 of 8 done; only the Attention queue and copy remain |
| Parent roadmap | [Plexmaton Roadmap](./plexmaton.md) |
| Product contract | [UI/UX](./ui-ux.md) |
| Depends on | Locked foundations in the parent roadmap |
| Unlocks | Phase 01 — Session and Provider Core; [math rendering track](./track-math-rendering.md) |
| Next step | Step 8 — Attention queue and selection/copy |

## Phase outcome

Produce a runnable, deterministic TUI prototype that demonstrates Plexmaton's defining multi-agent experience with synthetic agents and realistic load. The prototype must validate interaction and event boundaries without coupling them to unfinished provider, tool, mailbox, or persistence implementations.

At the exit gate, we should know that the proposed interface can remain understandable and responsive while several agents stream independently, and that the architecture can later replace synthetic producers with the real runtime without redesigning TUI state ownership.

## Canonical demonstration

The phase is organized around one end-to-end scenario:

1. The user converses with synthetic Agent A.
2. A streams text and delegates a task to synthetic Agent B.
3. B starts in the background while A and the composer remain interactive.
4. The user selects B and opens an inspector without losing A's scroll/focus state.
5. A and B stream concurrently into independent virtualized transcripts.
6. The user scrolls whichever surface is under the mouse without changing keyboard focus.
7. The user freely drags/resizes B, changes its z-order, pins or maximizes it, and returns to A.
8. B emits tool activity and an action-required event; it enters the Attention queue without opening a modal or stealing focus.
9. B emits a typed mail result and an artifact pointer.
10. The user inspects mail and artifact content, copies evidence, then returns to the exact prior surface states.
11. Terminal resize exercises wide, medium, and narrow layouts during activity.

No network or model API is required for this scenario. Synthetic timelines must be deterministic and replayable.

Math rendering was part of this scenario until 2026-08-31 and now belongs to the
[math rendering track](./track-math-rendering.md), which is blocked on this phase's viewport.

## Delivery sequence

The scope below is not a flat list. Each step needs the previous one to exist, and starting out of
order means building against a boundary that has not been decided yet.

1. **Intents and interaction router.** *Done 2026-08-31.* A typed `TuiIntent`, and one router that
   owns terminal event translation. Specified in
   [interaction-routing](../specs/interaction-routing.md).
2. **Surfaces on the application path.** *Done 2026-08-31.* Registration, surface kind, focus, and
   mouse capture. Specified in [surface-model](../specs/surface-model.md).
3. **The composer and the single cursor.** *Done 2026-08-31.* The one text input, its grapheme-aware
   editing model, and a submitted message as a runtime command (D-017, D-018, D-038). Added to this
   sequence on 2026-08-31 (D-037); the collapsed row (D-027) belongs to step 7 with the sub-agent
   input that triggers it.
4. **Viewports and scroll ownership.** *Done 2026-08-31.* Per-surface scroll state and the locked
   hover-routing and no-propagation rules from the UI/UX contract.
5. **Transcript virtualization.** *Done 2026-08-31.* Visible-range layout, a width-and-revision
   keyed wrapping cache, and semantic anchors. Specified in
   [transcript-layout](../specs/transcript-layout.md).
6. **Measurement harness.** *Done 2026-08-31.* Input-to-frame, scroll-to-frame, and layout work,
   before the workloads below can produce numbers worth recording. Specified in
   [frame-loop](../specs/frame-loop.md).
7. **Inspectors as shelves.** *Done 2026-08-31.* Shelf geometry and the ten-row guarantee, vertical
   resize with pointer capture, pin and maximize, boundary clamping (D-016, D-023, D-028), and the
   phase's one dismissible surface, so `Dismiss` and the `Escape` ladder landed here. Specified in
   [inspector](../specs/inspector.md).
8. **Attention queue and selection/copy.** Both depend on surfaces and viewports already existing.

Steps 1 to 5 are the interaction spine. A finding at any step that changes a durable invariant is
promoted to the parent roadmap or the UI/UX contract rather than recorded only here.

## Workspace and dependency skeleton

Phase 00 uses a deliberately small workspace:

```text
crates/
  plexmaton-core/   semantic prototype events, identities, and state-independent contracts
  plexmaton-tui/    reducer, SurfaceTree, interaction routing, transcript layout, and Ratatui rendering
  plexmaton-sim/    deterministic synthetic scenarios and workload generation
  plexmaton-cli/    terminal lifecycle and composition root for the runnable prototype
```

`plexmaton-core` must not depend on Ratatui, Crossterm, Tokio, or a math renderer. `plexmaton-tui` may depend on core but never the reverse. `plexmaton-sim` produces the same semantic boundary expected from the future runtime; the TUI may not call simulator objects directly. The CLI owns terminal initialization/restoration and task startup.

Dependency admission and the audited foundation live in
[standards/rust.md](../standards/rust.md); evidence tooling lives in
[standards/testing.md](../standards/testing.md). Both are triggered by the work, not by the phase,
so this section records only what stops being true when Phase 00 closes.

### Candidates behind adapters and features

| Candidate | Status | Constraint |
| --- | --- | --- |
| `arboard 3.6.1` | Local clipboard candidate | Evaluate Linux X11/Wayland feature and lifecycle behavior. It cannot be the only copy path because SSH/tmux/remote sessions may need OSC 52 or terminal-native selection |

`ratatui-textarea` left this table on 2026-08-31, rejected rather than deferred: it consumes
`crossterm::event::Event`, and exactly one component in this workspace may (D-038). Clipboard access
is an adapter (`ClipboardSink`), so semantic copy tests do not depend on the host desktop. That seam
does not exist yet; it is written before the crate is added, not after.

Math and image transport candidates moved to the [math rendering track](./track-math-rendering.md) on 2026-08-31.

Of the evidence tooling in [standards/testing.md](../standards/testing.md), `proptest` arrives with
clipping. Two scheduled tools reached their step and did not enter it, each because a stronger
instrument was available: `insta` at the viewport and virtualization steps, where a viewport
measured through the widget that paints it and a virtualized panel compared cell for cell against
the whole one both prove more than a snapshot; and `criterion` at the measurement harness, where the
load-bearing evidence is work counts it cannot see and the interesting statistic is a tail rather
than a mean (D-041). Each tool enters a manifest with the first test that needs it, never before,
and a schedule is a prediction rather than a commitment.

### Explicitly absent in Phase 00

- `reqwest`, `hyper`, TLS, and provider SDKs
- SQLite or another production store
- MCP client/server crates
- JavaScript, WebAssembly, or plugin runtimes
- Syntax-highlighting stacks
- General component frameworks layered over Ratatui
- Multiple terminal backends

Their boundaries may be represented by semantic test doubles, but their dependencies arrive only with the phase that owns them.

Entries age: once a later step lands, an earlier one keeps only what a future reader still needs —
corrections to claims made here, and findings that changed an invariant. The narration goes.

### Skeleton and tooling — 2026-08-30 to 2026-08-31 — compressed

- The four-crate workspace exists on Rust 2024 and project-local `1.98.0`, with the semantic
  boundary enforced by construction: no TUI-to-simulator call path exists. One Ratatui 0.30 and one
  Crossterm 0.29 generation, now machine-checked by `deny.toml` rather than by prose.
- **Correction.** The first PTY smoke claim was not reproducible as written and was retracted.
  `scripts/smoke-tui.py` replaced it with a repeatable command; the three properties that made the
  original wrong are recorded in
  [standards/quality-gates.md](../standards/quality-gates.md).
- Three documented behaviours had no implementation behind them and were corrected: the projection
  is genuinely revisioned and gates repaints on it; ordering defects degrade into a bounded typed
  notice log instead of terminating the workspace; and reduced-but-unrendered domain data — mail's
  sender among it — reaches the screen.
- Collections iterate in arrival order, not identifier order. A mutation check confirmed exactly
  two ordering tests fail when that is reverted.
- Splitting `state.rs` moved the per-agent invariants into `AgentView`, leaving the reducer owning
  only what is genuinely cross-agent: stream ordering, selection, and the notice log.
- `missing_docs` is denied in `plexmaton-core` only (D-010); workspace-wide it invited the restated
  signatures `AGENTS.md` rejects.

### Screen anatomy decisions landed — 2026-08-31 — compressed

Seven decisions, D-022 through D-028, worked out against ASCII compositions rather than a visual
mock; `ui-ux.md` and [DECISIONS.md](../DECISIONS.md) are the record and no external design document
is authoritative.

Two are implemented: `LayoutClass::for_size` selects five compositions with ultrawide at 132, and a
terminal below 48 × 12 renders one notice with a test asserting no workspace content leaks through.
The other five — shelf geometry, the ten-row guarantee, focus on open, the collapsed composer row,
and the reduced drag scope — remain unimplemented until the interaction spine reaches them.

### Delivery step 1 — intents and interaction router — 2026-08-31 — compressed

`plexmaton-tui::router` is the only place in the workspace that accepts a terminal event;
[`specs/interaction-routing.md`](../specs/interaction-routing.md) carries INV-1 to INV-9 and names
the test proving each. Three facts corrected earlier claims:

- **`Escape` no longer quits** (D-031). `Ctrl-C` is the unconditional exit; `q` quits only from a
  navigation surface, so it stays a letter while a text input holds the cursor.
- **Intents live in `plexmaton-tui`, not `plexmaton-core`** (D-030). The crate sketch above was
  corrected.
- **Declining an event is a named outcome.** `Routed::Ignored` carries a reason, so a key that does
  nothing is distinguishable in a test from a routing defect.

`Text` had no consumer and the sequence named no composer step. That gap is now delivery step 3
(D-037).

### Delivery step 2 — surfaces on the application path — 2026-08-31 — compressed

`SurfaceTree` stopped being a tested-but-unused structure. Layout is the only producer of workspace
rectangles, `render` draws by walking the registry and hands it back, and the executable routes
against the tree the last frame actually drew, so routing geometry cannot diverge from painted
geometry. Mouse reporting is on, released before the alternate screen and from the same guard that
restores it. Five facts a future reader still needs:

- **Surface identities are named, and behaviour is derived from one kind** (D-036). The draw loop is
  an exhaustive match, so a surface with no way to be drawn does not compile; and pointer
  eligibility, focusability and the text cursor come from `SurfaceKind` rather than from booleans
  that could disagree.
- **Focus is a preference resolved per frame, not a value repaired after layout.** The tree is
  rebuilt every frame, so a stored focus can name a surface that is not on screen. Resolving lazily
  leaves nothing to keep in sync and is what satisfies SURF-5; repairing would have meant mutating
  state during layout.
- **The focus ring wraps where the agent list clamps**, and its order is `SurfaceId` declaration
  order rather than geometric order, so it does not reorder itself when the terminal crosses a
  layout-class threshold.
- **Correction: the layout registered regions it had no room to draw.** At 48 × 12 the activity
  panel was registered `48x0` on top of the footer, and with a notice strip the agent rail collapsed
  to `48x1` — both invisible to the tiling test, because a zero-area rectangle covers nothing and
  intersects nothing. Ratatui's solver returns such a rectangle rather than failing. Rows are now
  reserved all-or-nothing in priority order, and a region that cannot clear three rows is not
  registered at all.
- **Two planned slices were cut, moving where two invariants are owned.** Clipping had no caller —
  every registered surface fits inside its parent — so SURF-2 belongs to step 8, where a transcript
  item scrolled past its viewport edge is the first surface that outgrows one. Modality has no owner
  in Phase 00 at all: the canonical journey never opens a modal, because the Attention queue exists
  so a background request does not, and a shelf overlays without blocking. `Dismiss` and the
  `Escape` ladder therefore land in step 7.

Recorded here and resolved in step 7: `ui-ux.md` calls Narrow "one major surface at a time" while
the implementation stacks bands. The bands stayed; what changed is that opening an inspector at
Narrow replaces the conversation rather than squeezing it.

### Delivery step 3 — the composer and the single cursor — 2026-08-31

The user can type a message and see it in the transcript. `KeyboardFocus::TextInput` is reachable in
the running binary, so INV-2 is no longer proven only against fixtures, and all four `TextIntent`
verbs have consumers. Every intent the router produces now has one except `Dismiss` and `Scroll`,
which belong to steps 7 and 4.

- **`ratatui-textarea` was rejected, not deferred** (D-038). Its API consumes
  `crossterm::event::Event`, and exactly one component in this workspace may. The editing model is
  four verbs, and `unicode-segmentation` and `unicode-width` — already audited into the foundation
  and until now inherited by no crate — are what those verbs actually need.
- **A submitted message is a command, not a write.** The projection never appends to its own
  transcript; text goes to `plexmaton-sim::Runtime` and reaches the screen as the events it emits
  back. That forced sequence numbering out of the scenario fixture and into the runtime, because two
  sources feeding one monotonic stream cannot both be numbering it — the projection rejects any gap
  or repeat, so interleaving had to be correct by construction.
- **The composer has no cursor offset.** The key grammar has no binding that moves a cursor, so the
  insertion point is always the end of the draft. A stored offset nothing can change would be a
  second thing able to disagree with the string.
- **The priority at the smallest terminal changed, and a test changed with it.** Twelve rows cannot
  hold a hint strip, a composer, a conversation, a notice strip and an agent rail. The rail is what
  yields: typing and the conversation are the workspace, and a producer defect the user cannot see is
  the failure D-003 exists to prevent, with no other signal for it. A step-2 assertion that the rail
  always survives a notice was true before the composer took three of those twelve rows, and has
  been replaced rather than relaxed.
- **`state/mod.rs` hit the 400-line sentinel and was split by responsibility**, not by raising the
  threshold: focus, the notice log, and the Attention queue each became a module owning its own
  invariants. The queue's coalescing rule — one entry per request identity — now has a home and a
  test before step 8 needs it.

Twelve mutations across steps 2 and 3 were each caught by the intended tests. One of them found a
hole in the tests rather than in the code: restoring a fixed four-row notice strip failed nothing,
because the only test covering it used the case where the strip does not compete for rows.

### Delivery step 5 — transcript virtualization — 2026-08-31

A frame lays out only the rows it draws, and a conversation remembers its reader by the message they
were on. [`specs/transcript-layout.md`](../specs/transcript-layout.md) carries TR-1 to TR-5. Three
defects the step-4 code had, each fixed and each now covered:

- **Every frame wrapped the whole history.** Heights are now measured per item and kept until that
  item's revision or the panel's width changes — a delta costs one wrap, a resize one pass, an
  unchanged frame none. `TranscriptMetrics::wrapped` is instrumentation that exists so the claim can
  be counted rather than asserted, and step 6's harness inherits it.
- **A resize moved the reader.** The step-4 test asserted that the stored *offset* survived a
  resize, which is the wrong property: at a new width the same row names different text. It has been
  replaced, not relaxed. A position is now an item plus a row inside it, and the row is clamped to
  that item's height at the current width — without the clamp a message that wrapped shorter is
  overshot and the next one appears at the top.
- **All conversations shared one reading position**, so canonical journey step 4 — select B, come
  back to A — lost A's place. Positions are keyed by agent.
- **Following the tail was a coincidence**, not a state. `offset == max_offset` stops being true the
  moment content arrives, so a reader at the newest line silently fell behind. `Tail` is now a
  stored arm, and scrolling back to the end re-arms it.

The strongest evidence here is differential rather than a snapshot: the virtualized panel is
compared cell for cell against one `Paragraph` holding the whole conversation, at three sizes and
four scroll positions. That proves virtualization changed the cost and not the picture, where a
snapshot would only prove the screen is stable. `insta` was scheduled for this step and did not
enter; it arrives with a test that needs it.

Two named scope items were cut with their reasons recorded in D-040: **bounded overscan**, which
prevents nothing in a synchronous renderer, and per-item **cache pruning**, which has nothing to
prune until something drops a transcript item.

`state/mod.rs` hit the 400-line sentinel again and was split rather than raised: `ingest.rs` now owns
the producer contract — stream ordering, the typed refusals, and event dispatch — leaving the rest
owning what the user is looking at.

### Delivery step 6 — measurement harness — 2026-08-31

`cargo run --release -p plexmaton-cli --bin plexmaton-measure` runs six workloads at two scales and
prints what each frame cost. [`specs/frame-loop.md`](../specs/frame-loop.md) carries FR-1 to FR-3,
and the budgets it produced are in [`ui-ux.md`](./ui-ux.md) §UX performance budgets — a table with
numbers in it, where the section had been a list of things to measure since the phase opened.

- **The loop became an object, and that is what made it measurable.** Redraw count is decided in the
  composition root, so while the loop was an `async fn` wrapped around a real terminal there was
  nothing to count. `plexmaton-tui::Workspace` now owns the projection, the router, the last frame's
  registry, the wrapping cache, and the painted revision; the executable keeps only what a process
  has, which is a terminal and an asynchronous wait. The harness substitutes a cell buffer and a
  scripted timeline and drives the same methods, so a measured frame is the frame the user gets.
- **Work is asserted; time is only reported** (D-041). Items wrapped, lines built, frames painted,
  and entries retained are the same on every machine and are ordinary tests, so a regression fails a
  build rather than a reading. Timings are recorded beside the machine and profile that produced
  them. A wall-clock assertion on a developer laptop is a flaky test wearing a budget's clothes, and
  the cure for the flakiness is a threshold that catches nothing.
- **Layout work is flat in history; total frame cost is not.** A steady frame wraps one item and
  builds twenty-seven lines whether the conversation holds five hundred messages or five thousand —
  which is the exit gate's full-history-rendering criterion, counted rather than argued. But the
  same frame still walks the item list five times to validate, sum, and locate, and that is what
  separates ~0.2 ms at five hundred from ~1 ms at five thousand. Linear in cheap operations, so it
  becomes the budget around fifty thousand messages. Recorded and deliberately not optimized: the
  measurement exists to say where to look first, and this is not yet it.
- **Resize is the expensive interaction.** Every height is width-dependent, so a new width
  re-measures every item once by design (TR-1) — 13 ms at five thousand messages, and 130 ms at
  fifty thousand. It is the first thing a retention limit or a lazily measured tail would be for.
- **Correction.** `transcript-layout` said the wrapping cache is held by the composition root. It is
  held by the frame loop, which the composition root owns; the row now points at the spec that has
  it.

Seven of the eight budget rows are filled. The eighth — surface open and close latency — needs a
surface that opens, and stays in the table naming delivery step 7 rather than being dropped.

Tests: 49 at the start of step 2, 113 now. All workspace gates, the supply-chain lane, and
`scripts/smoke-tui.py` pass.

### Delivery step 7 — inspectors as shelves — 2026-08-31

The workspace shows two agents at once and has its one dismissible layer.
[`specs/inspector.md`](../specs/inspector.md) carries INS-1 to INS-5, and `Dismiss` — the last
intent without a consumer since step 1 — has one. D-016, D-018, D-022, D-023, D-026, D-027 and
D-028 stop being decisions with no implementation.

Two questions `ui-ux.md` left open had to be answered before anything could be built, and both
changed what got built:

- **Inspection is an axis of its own** (D-042). Opening does not move the selection, so with one
  agent selected and another inspected there are two on screen — the only arrangement in which a
  shelf shows something the workspace does not already. It is also what gives **pinned** a meaning:
  an unpinned inspector *follows* the selection, and pinning is it declining to. The first
  implementation closed an unpinned inspector when the selection moved, which read correctly from
  the contract and was wrong in use — the peek ended at the moment it became useful.
- **The surface kind is `Inspector`, not `Shelf`.** `surface-model.md` predicted `Shelf`; a shelf is
  one of three presentations the same surface takes by terminal size, and a kind named after one
  geometry is the wrong name at the other two. `Modal` never arrived at all, for the reason SURF-4
  is still unproven: nothing in this phase blocks.

Findings and corrections:

- **Z-order promotion was cut, and the reason is that nothing overlaps.** A docked shelf splits the
  conversation region rather than covering it, so no two surfaces compete for a cell and no
  `z_index` above zero has a caller. That is the third Phase 00 mechanism to have no owner for the
  same reason, alongside SURF-2's clipping and SURF-4's modality.
- **Correction to `ui-ux.md`.** It gives the reason for the eighteen-row shelf cutoff as the ten-row
  guarantee failing. It does not fail — below eighteen the guarantee binds instead of the share, and
  the shelf shrinks while the conversation keeps its ten. What stops being true is that the shelf is
  worth being one. The number is unchanged; the reason is corrected in both files.
- **Two inputs made "exactly one cursor" a claim that could fail.** Until now the workspace had one
  text input, so COM-1 held by construction. Which agent a keystroke addresses is now derived from
  the focused surface, and a submission carries its target rather than being handed to whoever
  receives it — so the composition root stopped guessing the recipient.
- **A draft belongs to the conversation, not the surface.** Drafts are keyed by agent, which means
  peeking elsewhere and returning finds the half-written steer, and two inputs pointed at the same
  agent correctly show one draft.
- **Opening an inspector takes the arrows away from the agent rail**, because the inspector holds
  the cursor and an arrow under a cursor is not a list movement (INV-2). `Tab` gives them back,
  which is what the collapsed composer row advertises. Consistent, and worth knowing before using
  the binding.
- **Two files hit the 400-line sentinel and were split rather than raised**: `layout` into the
  workspace's row budget and where an inspector goes inside it, and `render` into what a surface
  draws and what it says about itself in its border.

Measured through step 6's harness: opening an inspector costs **zero** re-wrapping, because a shelf
splits the region vertically and the conversation keeps its width. That fills the eighth and last
`ui-ux.md` budget row. The same run showed the lane's own limit — every figure roughly doubled
against step 6's, and re-measuring the *previous commit* under the same load reproduced it, so the
spread is the machine. Both O(n) rows now read over budget on a loaded laptop and under it on a
quiet one; that is recorded rather than rounded away.

Tests: 49 at the start of step 2, 129 now. Eight mutations were each caught by the intended tests.
All workspace gates, the supply-chain lane, and `scripts/smoke-tui.py` pass.


## Scope

### Workspace skeleton

- Create the minimum Rust workspace boundaries needed to keep domain events, synthetic runtime, TUI state, and rendering separate.
- Keep crate boundaries small and justified; do not reproduce the reference project's single-crate module sprawl or create speculative crates for future features.
- Provide one runnable demo binary and one deterministic test/simulator entry point.

### Semantic prototype events

Define only the UI-facing semantic events needed by the scenario, such as:

- Agent created/state changed/completed
- Transcript item started/delta/finalized
- Tool activity changed
- Mail delivered/read
- Artifact announced
- Runtime warning/error

These are prototype contracts for the TUI boundary, not the final provider wire model or durable event schema. They need stable identities and revisions so replacement producers can update existing items without text matching.

### TUI application model

- Unidirectional update/effect/render flow
- Revisioned `ViewState`
- `SurfaceTree` with clipping and z-order
- Focus manager
- Mouse hit testing and local-coordinate translation
- Independent viewport state
- Full floating-inspector behavior: drag, resize, z-order promotion, pointer capture, boundary clamping, and responsive recovery
- Popup/peek, pinned pane, modal, popover, tooltip, and Attention queue primitives as required by the canonical journey
- Resize and responsive layout transitions
- Command routing and keyboard equivalents
- Semantic selection/copy independent of rendered cell decoration

### Transcript layout

- Item-based virtualization rather than one monolithic rendered string
- Width-aware wrapping cache keyed by item revision and layout width
- Visible-range calculation with bounded overscan
- Stable semantic scroll anchors
- Tail-follow state separate from raw scroll offset
- Incremental invalidation for streaming deltas
- Expand/collapse state for tool activity and artifacts

### Measurement harness

- Instrument input-to-frame, scroll-to-frame, redraw count, and layout work per updated block.
- Make the harness deterministic and runnable from a command, so evidence cites a command rather
  than an observation.
- Build it before the responsiveness workloads below, not after; budgets recorded without a
  credible harness are guesses.

### Testing and observation

- Ratatui buffer snapshots for stable visual states
- Deterministic event-sequence tests
- Mouse routing tests across overlapping surfaces and z-order changes
- Drag/resize pointer-capture tests, including overlap, terminal boundaries, and resize transitions
- Independent scrolling and focus restoration tests
- Attention-queue tests proving background requests do not steal focus or open modal surfaces
- Semantic selection/copy tests across decorated, clipped, and virtualized content
- Resize/anchor tests at representative terminal dimensions
- Large-transcript virtualization workload
- Concurrent synthetic-stream workload
- A repeatable interaction recording or scripted simulator trace for the canonical demonstration

## Proposed internal boundary

```text
SyntheticScenario
    -> semantic PrototypeEvent stream
       -> TuiReducer
          -> ViewState
             -> SurfaceTree layout + interaction registry
             -> virtualized transcript layouts
             -> Ratatui frame

TerminalEvent
    -> interaction router
       -> TuiIntent
          -> TuiReducer
```

The TUI must not call synthetic-agent objects directly. Synthetic producers and later real runtime producers meet at the semantic event/intent boundary.

## UI/UX decisions this phase must make

- Default wide, medium, and narrow screen anatomy
- What remains permanently visible versus progressively disclosed
- Agent selection, peek, pin, maximize, dismiss, and return behavior
- Floating-inspector drag, resize, placement, snapping, z-order, and boundary behavior
- Keyboard and mouse interaction grammar, with hover-routed wheel scrolling locked
- Nested-scroll propagation policy
- Focus restoration, Attention queue, and background notification behavior
- Application selection, terminal-native selection escape hatch, and semantic copy behavior
- Transcript visual grammar for messages, tools, mail, artifacts, errors, and math
- Initial design tokens and low-color degradation

Decisions are promoted into `ui-ux.md` with the evidence that motivated them.

## Responsiveness workloads

Measurements must include at least:

- One active transcript with rapid deltas
- Four agents with interleaved deltas and tool-state changes
- A large hidden transcript opened into an inspector
- Two visible independently scrolling transcripts
- Repeated wide/medium/narrow resize transitions
- A pending and then completed math render

Measure input-to-frame latency, scroll-to-frame latency, redraw frequency, layout work, inspector-open latency, and retained memory. Set initial budgets only after the measurement harness is credible.

## Explicit non-goals

- Real Chat Completions, Responses, or Messages requests
- Production session persistence
- Real shell/file tools
- MCP or plugin execution
- Durable mailbox delivery
- Provider authentication and model catalogues
- Full Markdown or TeX compatibility
- Final themes, branding, or animation polish
- A general desktop/windowing framework

Phase 00 may use interfaces shaped for later replacement, but it must not implement future subsystems behind fake abstractions.

## Risks and controls

| Risk | Control |
| --- | --- |
| Prototype becomes a throwaway visual mock | Route synthetic data through the intended semantic TUI boundary and test it deterministically |
| UI framework grows into a general widget toolkit | Implement only primitives required by the canonical journey |
| Layout is optimized only for one terminal size | Treat wide, medium, and narrow compositions as exit-gate evidence |
| Streaming causes full transcript re-layout | Instrument cache invalidation and visible-range work |
| Mouse behavior becomes widget-specific | Centralize hit testing, capture, focus, and propagation in the interaction router |
| Full floating windows expand into a desktop framework | Limit window behavior to agent inspectors and primitives required by the canonical journey |
| Background approvals interrupt the primary agent | Route action-required events through the Attention queue; prohibit background modal creation |
| Mouse capture breaks copy workflows | Test application-owned semantic selection plus terminal-native selection escape behavior |
| Beautiful screenshots hide poor interaction | Require scripted end-to-end behavior and latency evidence, not screenshots alone |
| Documentation outruns its evidence | Every evidence claim names the test or command that reproduces it |

## Exit gate

Phase 00 completes only when all of the following are demonstrated:

- The canonical A-to-B journey runs deterministically in the real TUI event loop.
- The user can continue interacting with A while B streams.
- Opening, scrolling, freely dragging/resizing, changing z-order, pinning/maximizing, closing, and reopening B preserve independent state.
- Mouse routing selects the correct topmost viewport in overlapping and nested cases; hover-scroll does not change keyboard focus.
- Drag and resize retain pointer capture, respect minimum sizes/bounds, and recover coherently across terminal resize.
- Background action-required events enter the Attention queue without stealing focus or opening a modal.
- Semantic copy returns underlying transcript, path, artifact, and equation source rather than decorated or truncated display cells.
- Keyboard-only navigation can perform the canonical journey.
- Wide, medium, and narrow layouts preserve the journey's meaning and viewport anchors.
- Large synthetic transcripts do not require full-history rendering for a frame.
- Snapshot and interaction tests cover the principal states and non-happy paths.
- Responsiveness measurements exist for the declared workloads, with initial budgets recorded in `ui-ux.md`.
- Durable UI/UX decisions discovered during the prototype have been promoted to `ui-ux.md`.

Passing compilation or showing a single polished screenshot does not satisfy the exit gate.

## Handoff to Phase 01

Before expanding Phase 01, record:

- The final semantic event/intent boundary expected from the runtime
- Which prototype event types remain and which were provisional
- The accepted responsive layouts and interaction grammar
- Transcript virtualization/cache invariants
- Initial performance budgets
- Whether the viewport is ready to unblock the [math rendering track](./track-math-rendering.md)

Phase 01 is then planned against evidence from this handoff rather than assumptions made before the TUI exists.
