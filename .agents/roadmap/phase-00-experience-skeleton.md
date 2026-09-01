# Phase 00 — Experience Skeleton

| Field | Value |
| --- | --- |
| Status | Delivery complete — 8 steps and one closure slice; the exit gate is assessed below |
| Parent roadmap | [Plexmaton Roadmap](./plexmaton.md) |
| Product contract | [UI/UX](./ui-ux.md) |
| Depends on | Locked foundations in the parent roadmap |
| Unlocks | Phase 01 — Session and Provider Core; [math rendering track](./track-math-rendering.md) |
| Next step | Phase 01, planned against the handoff below |

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
8. **Attention queue and selection/copy.** *Done 2026-08-31.* A visible, ordered band that takes
   nothing; a selection over content rather than cells; copy through a seam the workspace owns.
   Specified in [attention](../specs/attention.md) and
   [selection-and-copy](../specs/selection-and-copy.md).

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
| `arboard 3.6.1` | **Not adopted** (D-043) | The constraint recorded here answered the question: a local clipboard cannot be the only copy path, and OSC 52 — which Crossterm already has behind a feature — covers the remote case a native crate cannot. It waits for a user whose terminal refuses OSC 52 |

`ratatui-textarea` left this table on 2026-08-31, rejected rather than deferred: it consumes
`crossterm::event::Event`, and exactly one component in this workspace may (D-038). The clipboard
seam was written before any crate was added, and turned out to make one unnecessary: copy leaves the
workspace as a value on `Outcome`, so `plexmaton-tui` cannot reach a host clipboard even by mistake,
and `plexmaton-cli::clipboard::ClipboardSink` is what delivers it.

Math and image transport candidates moved to the [math rendering track](./track-math-rendering.md) on 2026-08-31.

Of the evidence tooling in [standards/testing.md](../standards/testing.md), `proptest` entered at
step 8 — with **selection**, not with clipping, which never found a caller. Two scheduled tools
reached their step and did not enter, each because a stronger instrument was available: `insta` at
the viewport and virtualization steps, where a viewport measured through the widget that paints it
and a virtualized panel compared cell for cell against the whole one both prove more than a
snapshot; and `criterion` at the measurement harness, where the load-bearing evidence is work counts
it cannot see and the interesting statistic is a tail rather than a mean (D-041). Each tool enters a
manifest with the first test that needs it, and a schedule is a prediction rather than a
commitment.

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


### Delivery step 8 — Attention queue and selection/copy — 2026-08-31

A background agent can ask for something without interrupting anyone, and the user can take evidence
out of the workspace. [`specs/attention.md`](../specs/attention.md) carries ATT-1 to ATT-3 and
[`specs/selection-and-copy.md`](../specs/selection-and-copy.md) carries SEL-1 to SEL-5. D-043, D-044
and D-045 record the three decisions that shaped it, and `ui-ux.md` lost its last three open
questions about selection and clipboards.

- **Arrows had to stop being a global binding first** (D-044, INV-10). They moved the agent
  selection from any navigational surface, which was defensible with one list on screen and stops
  being so with three. Now an arrow means "move inside what holds focus" — and that turned out to
  close a gap nobody had named: **nothing scrolled by keyboard at all**, so the wheel was the one
  interaction with no keyboard equivalent, which `ui-ux.md` §user control forbids.
- **A selection is a range over entries, never over cells** (D-043). Because it names content,
  scrolling, resizing and re-wrapping cannot change what it copies — which is not a mechanism that
  has to remember to extend past the viewport, but the absence of one. `proptest` entered here and
  is what makes that claim a statement about every width rather than about three of them.
- **The clipboard seam made the clipboard crate unnecessary.** Copy leaves as a value on `Outcome`,
  exactly as a submission does, so `plexmaton-tui` cannot reach a host clipboard even by mistake.
  What delivers it is OSC 52, which reaches the terminal the *user* is at rather than the desktop the
  *process* is on — the case `arboard` explicitly could not serve. Its honest cost: the terminal
  never acknowledges the sequence, so the workspace claims nothing about a copy having landed, and
  the selection staying visible is the whole of the feedback.
- **Correction: SURF-2's clipping has no owner in Phase 00 either.** Step 2 predicted this step would
  own it, because a transcript item scrolled past its viewport edge looked like a surface outgrowing
  its parent. It is not one — an item is content inside a surface, and TR-2 already owns which of its
  rows a frame builds. Layout tiles the terminal, so nothing overlaps and nothing overflows. That is
  now the third predicted mechanism to close the phase without a caller, alongside SURF-4's modality
  and z-order promotion, and all three for the same underlying reason.
- **The hint strip was silently losing `quit` on narrow terminals.** A one-row `Paragraph` clips its
  end, and the end is where `quit` was. It now sheds hints by rank until the rest fit, which is the
  same all-or-nothing discipline the row budget uses. Found by the existing narrow render test the
  moment two hints were added.
- **The measurement harness hit the 400-line sentinel and was split rather than raised**: what a
  measurement *is* — a frame, its work, a run's percentiles — from which situations are worth
  measuring, which is a reading of the responsiveness workloads rather than a mechanism.

Measured: extending a selection re-wraps **nothing**, because a selection changes a style and never a
character, so the heights the cache holds were measured unselected and stay valid. The full table is
in [`ui-ux.md`](./ui-ux.md); this run was on a quiet machine and every row is inside its budget,
including the two that read over on a loaded one at step 7. Both readings are kept, because the
spread between them is the point (D-041).

Tests: 49 at the start of step 2, 147 now. Eleven mutations were each caught by the intended tests.
All workspace gates, the supply-chain lane, and `scripts/smoke-tui.py` pass.

### Closure slice — corrections from an external review — 2026-09-01

An independent review of the delivered phase reported six defects. All six reproduced, four of them
against behaviour the workspace's own specs already forbade, and one against the phase's hardest
declared requirement. They are recorded here rather than folded into the steps that introduced
them, because what a future reader needs is not the fix but the class of thing eight steps of gates
did not catch.

- **The inspector never held a conversation.** `specs/inspector.md` opens by saying it must; what
  was built drew the inspected agent's tools, artifacts and mail — the same content the activity
  column already draws for the selected agent. So canonical step 5 had never run, neither had the
  two workloads §responsiveness workloads declares for it, and the surface-open budget was
  measuring a small detail panel. INS-6 now carries the contract. Making it a conversation was
  mostly plumbing, because heights were already keyed by agent and readers already belonged to
  conversations rather than to panels — but that is the point: **the cheap mechanism was in place
  and the surface that needed it was pointed somewhere else, and nothing failed.**
- **A hidden selection could copy the wrong agent's text.** The selection carried its agent, which
  was enough to stop a frame *highlighting* the wrong list and not enough to stop a copy *reading*
  one. The highlight disappearing is what made it invisible. Every path that changes what a surface
  shows now drops the selection rather than rebinding it (SEL-3).
- **Two documented boundary claims were unenforced.** A finalized transcript item still accepted
  deltas, and identities derived `Deserialize` straight onto the inner string, so `""` decoded into
  an `AgentId` the constructor refuses. Both are now refused the way every other producer defect is.
- **`touch()` was called on acceptance rather than on change**, so a runtime re-reporting a running
  agent or an executing tool repainted continuously — which FR-1 says explicitly must not happen.
  `select` had the same defect at the ends of a list, beside a `move_selection` that did not.
- **The composer measured itself in newlines while every panel wraps.** At 60 columns a
  150-character draft painted its tail ending at column 34 with the caret at column 59, on the
  border. It now wraps its own draft at the width it is drawn at, so the rows it asks for, the rows
  it paints and the row the caret lands on are one answer.
- **Three claims in this file and the README were wrong** and are corrected above: `Esc` had not
  quit since D-031, the journey's step-5 assertion claimed two different conversations while both
  panels showed agent B, and the exit gate said one declared workload had not run when three had
  not.

The common thread is worth more than the six fixes: **every one of these passed every gate.** Tests,
clippy, the sentinels, the citation checker and a PTY smoke test all held while the inspector showed
the wrong thing entirely. What caught them was somebody reading the code against the contract, which
is the one instrument this project does not own — and the second-order finding is that four of the
six were already written down as invariants, so the gates were not weak, they were simply not
pointed at the claims.

One consequence was recorded rather than fixed: an unpinned inspector open on the selected agent now
visibly shows that conversation twice. Pinning is the documented way out (INS-1), and whether
opening should pin by default is a question for real use.

Tests: 147 at the close of step 8, 158 now. Four mutations were each caught by the intended tests.
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

Assessed 2026-08-31, with the test or command that reproduces each claim. `plexmaton-tui::journey`
is the scripted canonical demonstration; everything it names runs under `cargo test --workspace`.

| Criterion | Evidence | Verdict |
| --- | --- | --- |
| The canonical A-to-B journey runs deterministically in the real TUI event loop | `journey::*`, driving the same `Workspace` the executable drives | Met |
| The user can continue interacting with A while B streams | `the_journey_reaches_two_agents_without_losing_the_first` | Met |
| Opening, scrolling, dragging/resizing, z-order, pinning/maximizing, closing and reopening B preserve independent state | `the_journey_pins_an_agent_and_takes_a_request_without_being_interrupted`, `dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle` | Met **except z-order**, which was cut because nothing overlaps |
| Mouse routing selects the correct topmost viewport; hover-scroll does not change keyboard focus | `wheel_routes_by_hover_and_never_changes_focus`, `the_wheel_falls_through_what_cannot_scroll_and_stops_at_what_is_merely_exhausted` | Met — but the **overlapping** case is proven against a fixture, because no real workspace surface overlaps another |
| Drag and resize retain pointer capture, respect bounds, and recover across resize | `dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle`, `a_dragged_height_is_clamped_rather_than_obeyed`, `a_drag_in_flight_survives_the_terminal_changing_size` | Met |
| Background action-required events enter the queue without stealing focus or opening a modal | `a_background_request_takes_no_focus_no_selection_and_no_cursor` | Met |
| Semantic copy returns underlying transcript, path, artifact and equation source | `copying_returns_the_source_between_the_endpoints`, `copying_an_artifact_returns_its_pointer_rather_than_its_label`, `a_selection_does_not_survive_the_surface_changing_agents` | Met for transcript, artifact and mail; **equations are out of the phase** (D-007) |
| Keyboard-only navigation can perform the canonical journey | `journey::*` presses nothing but keys except one deliberate wheel event | Met |
| Wide, medium and narrow layouts preserve the journey's meaning and viewport anchors | `the_journey_survives_wide_medium_and_narrow`, `a_resized_conversation_keeps_the_reader_on_the_same_message` | Met |
| Large synthetic transcripts do not require full-history rendering for a frame | `frame_work_is_bounded_by_the_viewport_and_not_by_the_history`, `scrolling_either_of_two_conversations_costs_no_measurement`; the harness reports 1 wrap and 27 lines at 5,000 messages, and 0 wraps with two conversations on screen | Met |
| Two agents stream concurrently into independent virtualized conversations | `two_conversations_scroll_independently_and_neither_moves_the_other`, `an_inspected_conversation_keeps_its_own_reading_position_across_a_close_and_reopen`, `the_journey_reaches_two_agents_without_losing_the_first` | Met — after the closure slice; the delivered step 7 inspector held a detail panel and this had never run |
| Snapshot and interaction tests cover the principal states and non-happy paths | 158 tests; `a_virtualized_conversation_paints_what_the_whole_one_did` is the differential that replaced snapshots | Met, without `insta` |
| Responsiveness measurements exist for the declared workloads, with budgets in `ui-ux.md` | `cargo run --release -p plexmaton-cli --bin plexmaton-measure`, ten workloads at two scales | Met |
| Durable UI/UX decisions have been promoted to `ui-ux.md` | D-013 to D-028 and D-041 to D-045 | Met |
| The producer boundary refuses what its own contract forbids | `a_delta_after_finalization_is_refused_and_the_text_does_not_land`, `an_identity_cannot_be_deserialized_past_its_constructor`, `a_repeated_status_or_tool_state_costs_no_frame` | Met — after the closure slice |

Two criteria are met with a named reduction rather than in full, and both reductions are recorded
where the mechanism would have lived. **Z-order** has no caller because layout tiles the terminal, so
no two surfaces ever compete for a cell; the same fact retired SURF-2's clipping and SURF-4's
modality. **Equation source** left this phase with the math track on 2026-08-31 (D-007), and the copy
model that would carry it — a selection over entries, each answering with its own semantic source —
is in place and needs one more entry kind rather than a new mechanism.

**Correction.** This section previously said one declared workload had not been run. Three had not:
a pending and then completed math render, which left with the math track (D-007); *a large hidden
transcript opened into an inspector*; and *two visible independently scrolling transcripts*. The
last two had no way to run, because the inspector held a detail panel rather than a conversation.
Both now have a workload and a row in the budget table, and the math render is the only one
outstanding.

## Handoff to Phase 01

### The semantic event and intent boundary

`plexmaton-core::PrototypeEvent` is what a runtime must produce, and `plexmaton-tui::TuiIntent` is
what the user produces. **They never merge**, and nothing in the TUI may call a runtime object
(D-030). Every variant survives a JSON round trip, and the tag is the stable external name — a
renamed variant is a wire break, and `the_event_tag_is_the_stable_external_name` is the canary.

The two things a real runtime must get right, both learned the hard way here:

- **One monotonic sequence, numbered by the producer.** The projection rejects any gap or repeat, so
  two sources feeding one stream cannot both number it — that is why numbering moved out of the
  scenario data and into `Runtime`.
- **Per-item revisions, continuous.** A delta must carry exactly the previous revision plus one, so
  a lost or duplicated update is detectable without comparing text.

### Which prototype events remain, and which were provisional

| Event | Status entering Phase 01 |
| --- | --- |
| `AgentCreated`, `AgentStatusChanged` | Durable |
| `TranscriptItemStarted`, `TranscriptDelta`, `TranscriptItemFinalized` | Durable; this is the shape the cache and the anchors are built on |
| `ToolActivityChanged` | Durable, and deliberately thin: no arguments, no output, no expand state |
| `AttentionRequested` | Durable one way only. It has no resolution counterpart, and Phase 01 owns inventing one (D-045) |
| `MailDelivered`, `ArtifactAnnounced` | Provisional. Both carry a bounded summary and a pointer, and neither has a body: [mailbox-delivery](../specs/mailbox-delivery.md) is written and unimplemented |
| `RuntimeWarning` | Durable as the degradation path (D-003) |

### Accepted layouts and interaction grammar

Five layout classes with ultrawide at 132, wide at 96, medium at 72, and a hard floor of 48 × 12
(D-024, D-025). The full key grammar is in [interaction-routing](../specs/interaction-routing.md),
[inspector](../specs/inspector.md) and [selection-and-copy](../specs/selection-and-copy.md); the two
rules a new binding must not break are that exactly one component accepts a terminal event, and that
`Escape` resolves exactly one layer per press.

### Transcript virtualization and cache invariants

[transcript-layout](../specs/transcript-layout.md) TR-1 to TR-5. The one a new producer can break
without noticing: **heights are keyed by item revision and panel width**, so an event that changes an
item's text without advancing its revision paints stale rows.

### Initial performance budgets

[`ui-ux.md`](./ui-ux.md) §UX performance budgets, reproduced by one command. Read the work columns as
contracts and the timings as a shape (D-041).

### The math rendering track

**Unblocked.** The viewport it was waiting for exists: a conversation is measured item by item and
built only where the viewport reaches, so an item whose height is not yet known — which is what a
pending render is — has a place to live. What it needs and does not have is an item kind that can
report a *provisional* height and invalidate it later; today a height changes only when a revision
does. That is the first thing [track-math-rendering](./track-math-rendering.md) has to design.

### What Phase 01 should not inherit uncritically

- `plexmaton-sim` is a stand-in with a scripted timeline. Its `RuntimeCommand` vocabulary has exactly
  one verb, and it is not a design for a real runtime's command surface.
- The Attention queue has no eviction. Nothing removes an entry, because nothing can resolve one.
- Nothing removes a transcript item, so cache pruning has never run (D-040).
