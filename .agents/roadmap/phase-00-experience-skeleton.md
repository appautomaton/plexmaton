# Phase 00 — Experience Skeleton

| Field | Value |
| --- | --- |
| Status | In progress — interaction spine started; step 1 of 7 complete |
| Parent roadmap | [Plexmaton Roadmap](./plexmaton.md) |
| Product contract | [UI/UX](./ui-ux.md) |
| Depends on | Locked foundations in the parent roadmap |
| Unlocks | Phase 01 — Session and Provider Core; [math rendering track](./track-math-rendering.md) |
| Next step | Delivery sequence step 2 — surfaces with clipping, focus, and modality |

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
2. **Surfaces with clipping.** Give `SurfaceTree` a clipping rectangle, focus, and modality, and
   put it on the application path. It is currently exercised only by its own unit test.
3. **Viewports and scroll ownership.** Per-surface scroll state and the locked hover-routing and
   no-propagation rules from the UI/UX contract.
4. **Transcript virtualization.** Visible-range layout, width-and-revision keyed wrapping cache,
   semantic anchors, and tail-follow separate from scroll offset.
5. **Measurement harness.** Input-to-frame, scroll-to-frame, and layout work, before the workloads
   below can produce numbers worth recording.
6. **Floating inspectors.** Drag, resize, pointer capture, z-order promotion, boundary clamping.
7. **Attention queue and selection/copy.** Both depend on surfaces and viewports already existing.

Steps 1 to 4 are the interaction spine. A finding at any step that changes a durable invariant is
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

### Dependency policy

- Rust 2024 on project-local toolchain `1.98.0` (current stable at scaffold planning time). Install/select it through `rust-toolchain.toml`; nightly is not a foundation requirement and no global rustup default is required.
- Declare shared versions in root `[workspace.dependencies]` and inherit them from member crates.
- Commit `Cargo.lock`; this workspace ships binaries.
- Treat the versions below as audited manifest baselines, not floating `latest` aliases. Cargo's normal compatible ranges remain useful; the committed lockfile records the exact resolved graph. Never use wildcard (`*`) requirements.
- Disable default features where they introduce unused backends, native libraries, formats, or runtimes.
- Keep core error types on `thiserror`; use `anyhow` only at application/composition boundaries.
- Add one dependency only for a current phase capability or measurement. Future provider/storage/plugin crates do not enter Phase 00.
- Experimental math, image, editor, and clipboard integrations sit behind narrow features/adapters and cannot leak their types into core contracts.
- Record license, Minimum Supported Rust Version (MSRV), transitive native dependencies, and terminal/backend compatibility before promoting a candidate to locked.

### Locked Phase 00 foundation

Dependency audit snapshot: 2026-08-30. Re-run resolution and compatibility checks when generating the first `Cargo.lock`; a newer release is adopted only after its changelog, features, resolved graph, and checks are reviewed.

| Crate | Audited baseline | Role | Feature/version decision |
| --- | --- | --- | --- |
| `ratatui` | `0.30.2` | Cell buffer, layout, text, widgets, test backend | Use the current modular Ratatui generation; do not downgrade for an experiment |
| `crossterm` | `0.29.0` | Terminal lifecycle and input events | Enable `event-stream`; add optional `osc52` only when that clipboard adapter is implemented; use one event-reader path |
| `tokio` | `1.53.1` | Async task/event runtime | No `full`; the current skeleton activates only `rt`, `macros`, and `time`. Add `sync` or `signal` only with their first real owner. This is the current line, not a fixed minor; `Cargo.lock` pins the resolved patch |
| `tokio-util` | `0.7.19` | Hierarchical cancellation | Defaults are empty; enable only `rt` for `CancellationToken`/child tokens |
| `futures-util` | `0.3.34` | Stream combinators | Prefer the focused utility crate over the `futures` umbrella; enable only features required by `StreamExt` and synthetic streams |
| `serde` / `serde_json` | `1.0.229` / `1.0.151` | Deterministic scenario and snapshot data | Enable Serde `derive`; serialization is not itself the final durable-session schema |
| `thiserror` | `2.0.20` | Library error types | No `anyhow::Error` in core contracts |
| `anyhow` | `1.0.104` | CLI/composition errors | Binary boundary only |
| `tracing` / `tracing-subscriber` | `0.1.44` / `0.3.23` | Structured diagnostics | Enable only required formatting/filtering layers; logs must be redirected away from the owned TUI screen |
| `unicode-width` | `0.2.2` | Terminal-cell measurement | Keep the CJK behavior explicit and test it; required for layout and hit-test correctness |
| `unicode-segmentation` | `1.13.3` | Grapheme-aware editing/selection | Unicode 17 line with MSRV 1.85; never index visible text by byte offset |

Prefer the umbrella `ratatui` application crate initially. Direct adoption of split `ratatui-core`, `ratatui-widgets`, and `ratatui-crossterm` is justified only by a measured compile-time or dependency-boundary benefit.

### Phase 00 candidates behind adapters/features

| Candidate | Status | Constraint |
| --- | --- | --- |
| `ratatui-textarea 0.9.2` | Composer spike | Maintained Ratatui fork; compatible with the modular 0.30 generation and adds grapheme-aware wrapping. Wrap it behind our composer intent/state boundary before deciding whether to keep it |
| `arboard 3.6.1` | Local clipboard candidate | Evaluate Linux X11/Wayland feature and lifecycle behavior. It cannot be the only copy path because SSH/tmux/remote sessions may need OSC 52 or terminal-native selection |

Composer state and commands are product contracts; no textarea crate may become the event-routing or focus authority. Clipboard access is an adapter (`ClipboardSink`), so semantic copy tests do not depend on the host desktop. Neither adapter seam exists yet; the seam is written before the crate is added, not after.

Math and image transport candidates moved to the [math rendering track](./track-math-rendering.md) on 2026-08-31.

### Dev-only evidence dependencies

| Crate | Role |
| --- | --- |
| `insta 1.48.0` | Ratatui buffer and serialized-state snapshots |
| `pretty_assertions 1.4.1` | Readable state/interaction diffs; never snapshot its human-oriented output |
| `proptest 1.11.0` | Geometry, clipping, scroll-anchor, resize, and routing invariants |
| `criterion 0.8.2` | Repeatable transcript-layout and interaction workloads when wall-time measurement is appropriate |

None of these are in a manifest yet, and that is deliberate: a dependency enters when the step that
needs it starts, not when it is planned. `insta` and `proptest` arrive with the surface and viewport
work; `criterion` arrives with the measurement harness.

Use Ratatui's `TestBackend` before adding a virtual-terminal dependency. Add PTY/VT emulation only when a test requires terminal escape-sequence behavior that the cell buffer cannot represent.

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

### Delivery step 1 — intents and interaction router — 2026-08-31

The interaction spine now has its first vertebra. `plexmaton-tui::intent` holds the typed
vocabulary and `plexmaton-tui::router` is the only place in the workspace that accepts a terminal
event. [`specs/interaction-routing.md`](../specs/interaction-routing.md) carries INV-1 to INV-9 and
maps each one to the test that proves it.

Three facts about this step are worth recording because they change earlier claims:

- **`Escape` no longer quits.** It resolves one interaction layer per press. `Ctrl-C` is the
  unconditional exit and `q` quits only from a navigation surface, so it stays a letter while a
  text input holds the cursor. Recorded as D-031; the footer hint and `smoke-tui.py` were corrected
  with it.
- **Intents live in `plexmaton-tui`, not `plexmaton-core`.** The crate sketch above previously
  listed them under core and has been corrected. D-030 records why.
- **Declining an event is a named outcome.** `Routed::Ignored` carries a reason, so a key that
  does nothing is distinguishable in a test from a routing defect.

The grammar is complete for every binding already locked in `ui-ux.md`; the consumers are not.
`Quit`, `MoveSelection`, and `TerminalResized` reach the workspace today. `CycleFocus`, `Dismiss`,
`Scroll`, `Pointer`, and `Text` are produced and tested but have no reducer until steps 2 to 4, and
the executable lists them explicitly rather than swallowing them in a wildcard. The surface tree
the router hit-tests against is still empty on the application path, so pointer events resolve to
`OutsideWorkspace` in the running binary; step 2 registers the regions.

Mutation checks confirm the new tests have teeth. Reordering the Escape ladder, re-hit-testing
during a drag instead of honouring capture, and letting `q` quit past a dismissible layer each fail
exactly the test that names the invariant, and reordering the ladder fails nothing else.

Tests: 34 to 49. All workspace gates, the supply-chain lane, and `scripts/smoke-tui.py` pass.

This is implementation evidence for the skeleton and step 1 only. It does not satisfy the Phase 00 canonical demonstration or exit gate.

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
