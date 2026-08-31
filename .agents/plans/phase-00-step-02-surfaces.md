# Plan — Phase 00, step 2: surfaces with clipping, focus, and modality

| Field | Value |
| --- | --- |
| Phase | [Phase 00](../roadmap/phase-00-experience-skeleton.md), delivery sequence step 2 |
| Contract | [interaction-routing](../specs/interaction-routing.md) for routing; the surface model is inline below until it earns a spec |
| Status | Not started — 0 of 5 slices |

## Outcome

`SurfaceTree` stops being a tested-but-unused structure and becomes the thing layout produces and
the router routes against. When the last slice lands, a click reaches the region that was actually
drawn, keyboard focus is a property of a surface rather than a two-state fact the caller asserts,
and the `CycleFocus`, `Dismiss`, and `Pointer` intents from step 1 have consumers.

## Contract (inline)

Promote to `specs/surface-model.md` only if these outlive the slices.

- **C-1 — Drawn equals registered.** A surface's registered rectangle is the rectangle the renderer
  draws into. Layout that computes a rectangle without registering it is a defect, because hit
  testing and painting then disagree and a click lands one panel over.
- **C-2 — Clip, not bounds.** Hit testing uses the intersection of a surface's bounds with its clip
  rectangle. A surface scrolled partly out of its parent is not hit where it is not visible.
- **C-3 — Focus is a surface property.** Exactly one surface holds keyboard focus. Whether a text
  cursor exists is derived from that surface's kind, never asserted independently.
- **C-4 — A modal blocks delivery below it.** Pointer and keyboard events do not reach surfaces
  beneath a modal, and hit testing stops at it rather than falling through.
- **C-5 — Hidden state survives.** A surface keeps its own focus and scroll state while covered,
  so re-opening it restores what the user left. Scroll arrives in step 3; the ownership is
  established here.

## Slices

**Slice 1 — the registration seam.** Layout produces named surfaces and registers them in a
`SurfaceTree` the renderer then draws from, replacing the bare `Rect`s `render.rs` computes per
frame. No routing behaviour changes yet.
*Proves C-1:* a test walks every registered surface and asserts the drawn region matches, so a
region that is laid out but not registered fails.
*Unblocks:* everything. Without it each later slice is tested against a tree nothing populates.

**Slice 2 — clipping.** `Surface` gains a clip rectangle; `hit_test` intersects bounds with clip.
*Proves C-2:* a child extending past its parent is hit inside the overlap and missed outside it.
*Unblocks:* focus and modality, both of which are meaningless if hit testing is wrong.

**Slice 3 — focus.** The tree gains a focused surface and an ordered focus ring including the
collapsed composer row (D-027). `CycleFocus` gets a consumer, a press focuses the surface it hits
and promotes its z-order, and `RouterContext::focus` is derived from the focused surface instead of
supplied by the caller.
*Proves C-3:* focus survives a covering surface opening and closing; a press on an unfocusable
surface does not move focus; hover still never does (INV-3).
*Unblocks:* modality, which is defined in terms of what focus and hit routing may reach.

**Slice 4 — modality and the dismissible stack.** `Surface` gains modality. Delivery below a modal
is blocked, and `RouterContext::dismissible` becomes a query over the tree rather than the
placeholder `bool` step 1 left. `Dismiss` gets a consumer.
*Proves C-4 and INV-6:* a pointer event over a covered surface does not reach it; `Escape` closes
exactly one layer and restores the focus that layer took.

**Slice 5 — turn the mouse on.** The CLI enables mouse capture. This is last because until slice 1
lands there is nothing for a pointer event to hit, and it is separate because it is the only slice
whose proof is not a unit test.
*Proves INV-8 end to end:* `scripts/smoke-tui.py` gains a case showing a click reaches a surface
and that a `Shift`-modified drag is left to the terminal's own selection.

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

- **Viewports and scroll ownership.** Step 3. C-5 establishes who owns the state; step 3 fills it.
- **Shelf geometry and the ten-row guarantee** (D-016, D-023). Needs viewports.
- **Shelf drag and resize** (D-028). Step 6.
- **The composer.** `Text` intents still have no consumer after this step. The delivery sequence
  does not currently name a composer step, which is a gap in the sequence rather than an omission
  here; it needs an owner before step 4 closes.
- **Attention queue and selection/copy.** Step 7.
