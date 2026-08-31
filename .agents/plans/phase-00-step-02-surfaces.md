# Plan — Phase 00, step 2: surfaces with clipping, focus, and modality

| Field | Value |
| --- | --- |
| Phase | [Phase 00](../roadmap/phase-00-experience-skeleton.md), delivery sequence step 2 |
| Contract | [surface-model](../specs/surface-model.md) SURF-1 to SURF-5; routing in [interaction-routing](../specs/interaction-routing.md) |
| Status | Slice 1 landed 2026-08-31 — 1 of 5 slices |

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

**Slice 2 — clipping.** `Surface` gains a clip rectangle and a `visible()` intersection; hit testing
and painting both use it.
*Proves SURF-2:* a surface extending past its clip is hit inside the overlap and missed outside it,
and a surface clipped to nothing is registered, painted as nothing, and unhittable.
*Unblocks:* focus and modality, both of which are meaningless if hit testing is wrong.

**Slice 3 — kind, and focus as a surface property.** `accepts_pointer` is replaced by a `kind` that
derives it along with focusability. The tree gains a focused surface and a deterministic focus ring;
`CycleFocus` gets a consumer, a press focuses the surface it hits, and `RouterContext::focus` is
derived from the focused surface's kind instead of supplied by the caller.
*Proves SURF-3:* the ring order is stable across frames, a press on an unfocusable surface does not
move focus, hover never does (INV-3), and focus held by a surface that stops being registered lands
on a real stop rather than dangling.
*Unblocks:* modality, defined in terms of what focus and hit routing may reach; and the composer,
which is a focus stop whose kind is what puts the cursor on screen.

**Slice 4 — modality and the dismissible stack.** A blocking kind joins the table. Delivery below a
blocking surface stops, and `RouterContext::dismissible` becomes a query over the tree rather than
the placeholder `bool` step 1 left. `Dismiss` gets a consumer.
*Proves SURF-4 and INV-6:* a pointer event over a covered surface does not reach it, the focus ring
contains only the modal, and `Escape` closes exactly one layer and restores the focus it took.

**Slice 5 — turn the mouse on.** The CLI enables mouse capture and releases it on every exit path,
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

After that the order is forced: clipping before focus and modality, because both are defined in
terms of what a pointer can reach; focus before modality, because modality is a restriction on
focus and delivery; the terminal last, because it is the only step that cannot be proven
hermetically.

## Deliberately not in this plan

- **The composer.** Delivery step 3, which this step's focus ring is a prerequisite for. Slice 3
  adds the kind that will carry the cursor; it does not add an input.
- **Viewports and scroll ownership.** Step 4. SURF-5 establishes who owns the state; step 4 fills it.
- **Shelf geometry, the ten-row guarantee, and shelf resize** (D-016, D-023, D-028). Step 7. Slice 4
  gives them modality; it does not give them geometry.
- **Attention queue and selection/copy.** Step 8.
