# Phase 00 — Experience Skeleton

| Field | Value |
| --- | --- |
| Status | In progress — skeleton verified and tightened; interaction spine not started |
| Parent roadmap | [Plexmaton Roadmap](./plexmaton.md) |
| Product contract | [UI/UX](./ui-ux.md) |
| Depends on | Locked foundations in the parent roadmap |
| Unlocks | Phase 01 — Session and Provider Core; [math rendering track](./track-math-rendering.md) |
| Next step | Delivery sequence step 1 — intents and interaction router |

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

1. **Intents and interaction router.** A typed `TuiIntent`, and one router that owns terminal event
   translation. Until this exists, every later interaction is wired ad hoc into a widget.
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
  plexmaton-core/   semantic prototype events, identities, intents, and state-independent contracts
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

### Verified skeleton evidence — 2026-08-30

- The four-crate workspace now exists with Rust 2024 and project-local Rust `1.98.0`: semantic contracts in `plexmaton-core`, deterministic timelines in `plexmaton-sim`, view/reducer/surface/rendering in `plexmaton-tui`, and terminal lifecycle plus the async loop in `plexmaton-cli`.
- The runnable `plexmaton` binary consumes the simulator only through ordered `PrototypeEventEnvelope` values and the TUI consumes only those semantic events; no TUI-to-simulator call path exists.
- The first `Cargo.lock` resolves 98 external packages compatible with Rust 1.98.0 (102 lock entries including the four workspace packages). There is one Ratatui 0.30 generation and one Crossterm 0.29 generation. `cargo tree -d` reports only transitive implementation generations (`hashbrown` and `syn`), not duplicate terminal foundations.
- Feature inspection removed unused Tokio `sync` and `signal` activation. The executable currently activates `macros`, `rt`, and `time`; audited workspace dependencies that are not yet inherited by a member do not enter the resolved graph.
- Eight deterministic tests cover identifier validation, scenario repeatability/order, reducer selection and sequence failure, z-ordered hit testing/promotion, structural TestBackend rendering, and explicit quit keys.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` pass on the initial skeleton.

### Skeleton tightening — 2026-08-31

Three documented behaviors had no implementation behind them and were corrected.

- **The projection is now genuinely revisioned.** `ViewState::revision` advances on every visible
  change and holds on a no-op, and the executable repaints only when it advances. A PTY run shows
  14 painted frames for 14 events instead of a repaint per loop iteration. Covered by
  `revision_advances_on_visible_change_and_holds_on_a_no_op` and
  `only_resize_invalidates_the_painted_frame`.
- **Ordering defects no longer terminate the workspace.** `ViewState::apply` returns
  `ApplyOutcome` and records a typed `NoticeView`. A forward gap resynchronizes, a stale sequence
  is dropped without rewinding, and a contract violation drops one event without blocking the
  stream. The notice log is bounded at 32 entries and reports how many it discarded. Covered by
  four reducer tests.
- **Reduced-but-unrendered domain data is now rendered.** Mail retains its sender, which the
  reducer previously discarded; tool activity, artifacts, mail, and notices have render paths and
  tests. Collections iterate in arrival order rather than identifier order; a mutation check
  confirmed the two ordering tests fail when that is reverted, and no others do.

Twenty-one tests pass. Three layout classes are implemented in `LayoutClass::for_width` with
provisional thresholds recorded in the UI/UX contract.

**Correction to the previous entry.** The earlier PTY smoke claim above could not be reproduced as
written: `script` allocates a pseudo-terminal with no window size, the frame renders into a 0x0
viewport, and nothing is painted. `scripts/smoke-tui.py` replaces the claim with a repeatable
command that sets the window size explicitly, asserts the painted frame, forces a full repaint
through a real `SIGWINCH` resize, and checks that the alternate screen is released. It requires a
controlling terminal for the child, without which the resize is silently ignored.

Lint and format policy was tightened at the same time: `rustfmt.toml` and `clippy.toml` are pinned,
and the workspace now denies `print_stdout`, `print_stderr`, `unwrap_used`, `too_many_lines`, and
`cognitive_complexity`. `missing_docs` is denied in `plexmaton-core` only, because that crate is the
semantic contract; enforcing it workspace-wide invited restated signatures, which AGENTS.md rejects.

### Tooling lanes — 2026-08-31

- `deny.toml` makes the dependency rules machine-checked. The ban on duplicate `ratatui` and
  `crossterm` generations was previously prose plus a manual `cargo tree -d`; it now fails a build.
  The licence allow-list is exactly the five licences present in the resolved graph rather than a
  wishlist, so a new dependency carrying anything else stops for review.
- `cargo machete` reports declared-but-unused dependencies; the workspace is currently clean.
- `typos` covers prose and identifiers, with `_typos.toml` holding proper nouns.
- `scripts/check-file-length.sh` adds a 400-line file-level sentinel measured above the first
  `#[cfg(test)]` module. It was verified to fire by running it at a lowered threshold.
- `.githooks/pre-commit` runs the fast gates locally; `.github/workflows/ci.yml` runs everything
  including the pseudo-terminal smoke.

`state.rs` was split into `state/{mod,agent,ordered}.rs` as part of adopting the sentinel, and the
per-agent invariants — transcript item identity and per-item revision continuity — moved from
`ViewState` into `AgentView`. The reducer is now left owning only what is genuinely cross-agent:
stream ordering, selection, and the notice log. Tests went from 21 to 26.

This is implementation evidence for the skeleton only. It does not satisfy the Phase 00 canonical demonstration or exit gate.

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
