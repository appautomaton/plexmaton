# Plan — Phase 00, step 2: surfaces with clipping, focus, and modality

| Field | Value |
| --- | --- |
| Phase | [Phase 00](../roadmap/phase-00-experience-skeleton.md), delivery sequence step 2 |
| Contract | [surface-model](../specs/surface-model.md) SURF-1 to SURF-5; routing in [interaction-routing](../specs/interaction-routing.md) |
| Status | Slices 1 and 2 landed 2026-08-31 — 2 of 4 |

## Outcome

`SurfaceTree` stops being a tested-but-unused structure and becomes the thing layout produces and
the router routes against. When the last slice lands, a click reaches the region that was actually
drawn, keyboard focus is a property of a surface rather than a two-state fact the caller asserts,
and the `CycleFocus`, `Dismiss`, and `Pointer` intents from step 1 have consumers.

The contract these slices prove was inline here until 2026-08-31 and is now
[specs/surface-model.md](../specs/surface-model.md), because five later delivery steps are built
against it and a consumed plan cannot hold a contract that outlives it.

## Slices

**Slice 1 — the registration seam.** *Done 2026-08-31.* `layout::workspace` registers every region;
`render` draws by walking the tree and returns it; the executable routes against the tree the last
frame drew. SURF-1 proven twice — signatures read back through each registered rectangle, and a
tiling check that fails on a region laid out but never registered. `SurfaceId` became a named enum
(D-036).

**Slice 2 — kind, and focus as a surface property.** *Done 2026-08-31.* `accepts_pointer` became a
`SurfaceKind` that derives it along with focusability and the cursor; the tree gained a focus ring;
`ViewState` holds focus as a preference resolved against the current tree; `CycleFocus` and
`Press` gained consumers; `RouterContext::focus` is derived rather than supplied. SURF-3 proven at
four levels, and SURF-5 proven for focus by lazy resolution rather than by a repair step.

**Slice 3 — modality and the dismissible stack.** A blocking kind joins the table. Delivery below a
blocking surface stops, and `RouterContext::dismissible` becomes a query over the tree rather than
the placeholder `bool` step 1 left. `Dismiss` gets a consumer.
*Proves SURF-4 and INV-6:* a pointer event over a covered surface does not reach it, the focus ring
contains only the modal, and `Escape` closes exactly one layer and restores the focus it took.

**Slice 4 — turn the mouse on.** The CLI enables mouse capture and releases it on every exit path,
including panic. This is last because until slice 1 landed there was nothing for a pointer event to
hit, and it is separate because it is the only slice whose proof is not a unit test.
*Proves INV-8 end to end:* `scripts/smoke-tui.py` gains a case showing a click reaches a surface,
that a `Shift`-modified drag is left to the terminal's own selection, and that the terminal is
restored with mouse reporting off.

## Order and why

Registration first, deliberately. Step 1 already produced five intents with no consumer; doing the
model work before the seam would repeat that at larger scale and leave four slices of clipping and
focus logic proven only against fixtures. With the seam first, every later slice has a real caller
on the day it lands.

After that the order is forced: focus before modality, because modality is a restriction on focus
and delivery; the terminal last, because it is the only step that cannot be proven hermetically.

## Deliberately not in this plan

- **Clipping (SURF-2).** This step planned a clipping slice and does not contain one. Every surface
  the workspace registers fits inside its parent, so a clip rectangle would have had no caller and
  `visible()` would have returned `bounds` at every call site — the speculative abstraction
  `AGENTS.md` rejects, and a repeat of the step-1 mistake of shipping something with no consumer.
  The first surface that genuinely outgrows its parent is a transcript item scrolled past its
  viewport edge, which needs items to be hit targets; SURF-2 is therefore owned by step 8, and its
  evidence row names that step. If a viewport in step 4 produces a clipped surface sooner, it moves
  sooner — this is a plan, and finding the order is what it is for.

- **The composer.** Delivery step 3, which this step's focus ring is a prerequisite for. Slice 3
  adds the kind that will carry the cursor; it does not add an input.
- **Viewports and scroll ownership.** Step 4. SURF-5 establishes who owns the state; step 4 fills it.
- **Shelf geometry, the ten-row guarantee, and shelf resize** (D-016, D-023, D-028). Step 7. Slice 4
  gives them modality; it does not give them geometry.
- **Attention queue and selection/copy.** Step 8.
