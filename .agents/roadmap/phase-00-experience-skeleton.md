# Phase 00 — Experience Skeleton

| Field | Value |
| --- | --- |
| Status | In progress — interaction spine started; steps 1 and 2 of 8 complete |
| Parent roadmap | [Plexmaton Roadmap](./plexmaton.md) |
| Product contract | [UI/UX](./ui-ux.md) |
| Depends on | Locked foundations in the parent roadmap |
| Unlocks | Phase 01 — Session and Provider Core; [math rendering track](./track-math-rendering.md) |
| Next step | Step 3 — the composer and the single cursor (D-037) |

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
2. **Surfaces on the application path.** Registration, surface kind, focus, and mouse capture.
   Specified in [surface-model](../specs/surface-model.md).
3. **The composer and the single cursor.** The one text input, its grapheme-aware editing model, and
   the collapsed row (D-017, D-018, D-027). Added to this sequence on 2026-08-31 (D-037).
4. **Viewports and scroll ownership.** Per-surface scroll state and the locked hover-routing and
   no-propagation rules from the UI/UX contract.
5. **Transcript virtualization.** Visible-range layout, width-and-revision keyed wrapping cache,
   semantic anchors, and tail-follow separate from scroll offset.
6. **Measurement harness.** Input-to-frame, scroll-to-frame, and layout work, before the workloads
   below can produce numbers worth recording.
7. **Inspectors as shelves.** Shelf geometry and the ten-row guarantee, vertical resize with pointer
   capture, z-order promotion, pin and maximize, boundary clamping (D-016, D-023, D-028). The shelf
   is the phase's one dismissible surface, so `Dismiss` and the `Escape` ladder land here.
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
| `ratatui-textarea 0.9.2` | Composer spike | Maintained Ratatui fork; compatible with the modular 0.30 generation and adds grapheme-aware wrapping. Wrap it behind our composer intent/state boundary before deciding whether to keep it |
| `arboard 3.6.1` | Local clipboard candidate | Evaluate Linux X11/Wayland feature and lifecycle behavior. It cannot be the only copy path because SSH/tmux/remote sessions may need OSC 52 or terminal-native selection |

Composer state and commands are product contracts; no textarea crate may become the event-routing or focus authority. Clipboard access is an adapter (`ClipboardSink`), so semantic copy tests do not depend on the host desktop. Neither adapter seam exists yet; the seam is written before the crate is added, not after.

Math and image transport candidates moved to the [math rendering track](./track-math-rendering.md) on 2026-08-31.

Of the evidence tooling in [standards/testing.md](../standards/testing.md), `proptest` arrives with
clipping, `insta` with the viewport and virtualization steps, and `criterion` with the measurement
harness. Each enters a manifest with the first test that needs it, never before.

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

### Delivery step 2, slice 1 — the registration seam — 2026-08-31

Layout is now the only place that computes a workspace rectangle, and every rectangle it computes
is registered. `plexmaton-tui::layout::workspace` returns a `SurfaceTree`; `render` draws by walking
that tree and returns it; the executable routes against the tree the last frame actually drew. The
`SurfaceTree` is no longer a tested-but-unused structure, and pointer events in the running binary
now resolve to a real region instead of `OutsideWorkspace`.

Two facts worth recording:

- **Surface identities are named, not numbered** (D-036). `SurfaceId` became an enum, so the draw
  loop is an exhaustive match: a new surface cannot be added without stating how it is drawn.
- **Routing geometry is the geometry that was painted.** `render` hands its registry back rather
  than letting the caller recompute one. A second layout computed for hit testing is how a click
  lands one panel over, and this removes the possibility rather than testing for it.

SURF-1 has two proofs. `every_registered_surface_is_drawn_inside_its_own_bounds` reads the painted
cells back through each registered rectangle and asserts that surface's signature is inside it;
`registered_surfaces_tile_the_terminal_without_gaps_or_overlap` fails on a region that was laid out
but never registered. Mutation checks: drawing the agent rail into the transcript's rectangle fails
the first, and computing the activity region without registering it fails the second.

The test scaffolding changed with it. `canonical_state()` had been copied into two files and now has
one home in `test_support`, alongside `region_text`, which is what makes "drawn equals registered"
readable back out of a cell buffer. `plexmaton-core` gained a round-trip test over every event
variant plus one pinned wire tag; a renamed variant fails only the tag test, which is the point,
since a round trip cannot see a rename. `serde_json` entered as a dev-dependency with that test.

Tests: 49 to 57. All workspace gates, the supply-chain lane, and `scripts/smoke-tui.py` pass.

### Delivery step 2, slice 2 — kind, and focus as a surface property — 2026-08-31

`Surface::accepts_pointer` was a boolean, and focus needed a second one. Both are now derived from
one `SurfaceKind`, so a chrome strip that holds the cursor or a focus stop the pointer cannot reach
are unrepresentable rather than merely untested. `KeyboardFocus` moved out of the router and beside
the kind that decides it, which is what makes SURF-3's "never asserted independently" structural:
the executable no longer names a focus, it asks the focused surface's kind.

Three findings:

- **Focus is a preference, resolved per frame, not a value repaired after layout.** The tree is
  rebuilt every frame, so a stored focus can name a surface that is not on screen. Resolving it
  lazily — the stored stop if it is still registered, otherwise the first — means there is nothing
  to keep in sync, and SURF-5 falls out of it: a surface that comes back gets its focus back. The
  repair alternative would have had to mutate state during layout, which the anti-pattern list
  forbids.
- **The ring wraps where the agent list clamps**, and the contrast is deliberate. A held arrow key
  that wraps sends the user back to the first agent at the moment they stop reading; a `Tab` that
  stops cycling is a dead key with no way to discover why.
- **Ring order is `SurfaceId` declaration order, not geometric order.** A ring that reorders itself
  when the terminal crosses a layout-class threshold costs exactly the muscle memory it exists to
  build. The enum is declared in reading order, so at every class the two agree anyway — asserted
  across all five.

**Planned and not done: clipping.** The step had a clipping slice; it was cut on inspection, because
every surface the workspace registers fits inside its parent, so `visible()` would have returned
`bounds` at every call site. SURF-2 stays in the spec, listed as unproven and owned by step 8, where
a transcript item scrolled past its viewport edge is the first surface that genuinely outgrows its
parent. Shipping the field early would have repeated step 1's own mistake at model level.

Focus is proven at four levels — kind, tree, reducer, and painted cells — and read back from the
buffer against all three palettes, so a monochrome terminal must show focus too. Six mutations were
each caught by the intended tests: chrome made focusable (5 failures), the fallback removed (5), the
stored preference discarded (5), every panel painted focused (1), the ring clamped (2), and hit
testing ignoring kind (1).

Tests: 57 to 68. All workspace gates, the supply-chain lane, and `scripts/smoke-tui.py` pass.

### Delivery step 2, slice 3 — the mouse is on — 2026-08-31

The router had translated pointer events since step 1 against a terminal that was never asked to
send any. The executable now enables mouse reporting and releases it from the same guard that
restores the screen, so the release also covers the error and panic paths.

Releasing capture is ordered *before* the alternate screen is handed back. The other order switches
the modes off on the terminal the user is already looking at, and a leaked capture is worse than a
leaked screen: the shell keeps reporting movement with nothing left to explain why.

`scripts/smoke-tui.py` grew the part `TestBackend` cannot hold. It sends a real SGR press so
crossterm's parser is on the path, and asserts by *contrast*: a press on the transcript repaints,
and a press on the key-hint strip repaints nothing. The negative half is what makes it a routing
test rather than an "input causes redraw" test, and it is only meaningful because the deterministic
timeline has drained by then, so nothing else can repaint. Three mutations each fail it: capture
never released, capture never enabled, and chrome made interactive.

**Step 2 is complete at three slices, not five.** Clipping and modality were both planned here and
both cut, for one reason — this is the step where `SurfaceTree` gains real callers, so a field or a
variant with no caller belongs to the step that earns it. Clipping moves to step 8, where a
transcript item scrolled past its viewport edge is the first surface that outgrows its parent.
Modality has no owner in Phase 00 at all: the canonical journey never opens a modal, because the
Attention queue exists so that a background request does not, and the shelf overlays without
blocking. `Dismiss` and the `Escape` ladder therefore land in step 7 with the shelf, and SURF-4 says
in its evidence row that nothing in this phase proves it.

Tests: 68, unchanged — this slice's proof is a command, not a unit test.

This is implementation evidence for the skeleton and steps 1 to 2 only. It does not satisfy the Phase 00 canonical demonstration or exit gate.

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
