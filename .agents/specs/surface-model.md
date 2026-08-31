# Spec — Surface model

| Field | Value |
| --- | --- |
| Status | Partially implemented; SURF-1 and SURF-3 proven, SURF-5 proven for focus |
| Owns | What a surface is, how one is registered, and which surface an event may reach |
| Depends on | The surface categories and routing rules in [`ui-ux.md`](../roadmap/ui-ux.md) |
| Proven by | `plexmaton-tui::layout`, `::surface`, and `::render` tests; see the evidence table |

## Purpose

Every interactive region in the workspace is a surface. This file defines what one is and which
surface an event is allowed to reach; [`interaction-routing`](./interaction-routing.md) defines how
a terminal event becomes an intent in the first place. The two meet at `SurfaceTree`, which the
router borrows and never mutates.

Without one registry, geometry gets recomputed per widget, and painting and hit testing drift until
a click lands one panel over — a defect visible only to whoever is clicking.

## Invariants

**SURF-1 — Drawn equals registered.** Layout is the only producer of workspace rectangles, and every
rectangle it computes is registered. The renderer draws each surface only inside that surface's
visible rectangle. A rectangle computed without being registered is a defect, not an omission.

**SURF-2 — Clip, not bounds.** A surface carries both the rectangle it occupies and the rectangle it
is clipped to. Hit testing and painting use their intersection. A surface scrolled or covered past
its parent's edge is not hit where it is not visible, and its content keeps its logical origin
rather than restarting at the visible slice.

**SURF-3 — Focus is a surface property.** At most one surface holds keyboard focus, and it is a
surface the focus ring contains. Whether a text cursor exists is derived from the focused surface's
kind, never asserted independently, because two independent answers is how the workspace ends up
with none or two cursors.

**SURF-4 — A modal blocks delivery below it.** While a surface that blocks is registered, pointer
hit testing stops at it and the focus ring contains only it. Events do not fall through to what it
covers.

**SURF-5 — Hidden state survives.** A surface's own focus and scroll state belong to the surface, not
to the frame that drew it. Covering, unregistering for a frame, or re-registering a surface does not
reset them, so re-opening it restores what the user left.

## Model

```text
layout::workspace(area, …) ─▶ SurfaceTree ─▶ render draws each surface into visible()
                                   │
                                   └─▶ RouterContext borrows it for hit testing
```

### The surface record

| Field | Meaning |
| --- | --- |
| `id` | `SurfaceId`, a named variant. Never a number agreed by convention (D-036) |
| `bounds` | The rectangle the surface occupies, whether or not all of it is visible |
| `clip` | The rectangle it is confined to, normally its parent's visible rectangle. Not a field yet; it arrives with SURF-2's first caller |
| `z_index` | Draw and hit order among siblings |
| `kind` | What the surface *is*; every behavioural answer below is derived from it |

`visible()` is `bounds ∩ clip` and is what both painting and hit testing use (SURF-2). Until a
surface exists that does not fit inside its parent, `bounds` is that rectangle and there is no clip
field to disagree with it.

### Kind, and why the behaviour is derived

Whether a surface takes the pointer, can hold focus, holds the text cursor, blocks what is beneath
it, and answers `Escape` are five questions with one answer each. Storing them as five booleans
permits sixteen combinations that mean nothing — a chrome strip that holds the cursor, a modal that
does not block. Deriving all five from `kind` makes those states unrepresentable.

| Kind | Pointer | Focusable | Cursor | Blocks below | Dismissible |
| --- | --- | --- | --- | --- | --- |
| `Panel` | yes | yes | no | no | no |
| `Chrome` | no | no | no | no | no |

Later kinds join this table in the step that earns them: `Composer` with the composer, `Shelf` and
`Modal` with inspectors. A kind with no surface using it is not added in advance.

### Hit testing

Among surfaces whose `kind` takes the pointer and whose `visible()` contains the point, the one with
the greatest `(z_index, id)` wins. A blocking surface truncates the search rather than being merely
topmost, so a hole in a modal is not a hole in its modality.

### Focus

The focus ring is the registered focusable surfaces in a deterministic order, so keyboard-only
navigation is inspectable rather than emergent (`ui-ux.md` §user control). A press focuses the
surface it hits; hover never does, per [`interaction-routing`](./interaction-routing.md) INV-3.

## Failure modes

| Situation | Response |
| --- | --- |
| A second surface registered under one identity | `SurfaceTreeError::DuplicateSurface`; registration is a defect, not a silent overwrite |
| A surface whose `visible()` is empty | Registered, painted as nothing, unhittable. It exists so its retained state survives (SURF-5) |
| Focus held by a surface no longer registered | Focus moves to the first ring stop; it never becomes a dangling identity |
| A press on a surface that is not focusable | The press routes, focus does not move |
| A terminal below the supported minimum | Nothing is registered, so a pointer event resolves to nothing rather than to a guess (D-025) |
| Z-order exhausted by promotion | `SurfaceTreeError::ZOrderExhausted`; never a silent wrap that reorders the workspace |

## Out of scope

- **Terminal-event translation, capture, and the `Escape` ladder.**
  [`interaction-routing`](./interaction-routing.md) owns them.
- **What a viewport does with a scroll intent**, including the no-propagation rule (D-006). Delivery
  step 4 owns the mechanism; the rule is locked in `ui-ux.md`.
- **Shelf geometry and the ten-row guarantee** (D-016, D-023). Stated in `ui-ux.md`; implemented in
  delivery step 7.
- **Which surfaces exist.** That is layout's decision, and it changes with the layout class.

## Evidence

| Invariant | Proven by |
| --- | --- |
| SURF-1 | `every_registered_surface_is_drawn_inside_its_own_bounds`, `registered_surfaces_tile_the_terminal_without_gaps_or_overlap` |
| SURF-2 | Unproven — delivery step 8, the first step with a surface that outgrows its parent |
| SURF-3 | `chrome_is_neither_a_pointer_target_nor_a_focus_stop`, `the_focus_ring_wraps_in_both_directions`, `focus_outside_the_ring_enters_it_from_the_matching_end`, `the_focus_ring_is_the_three_panels_at_every_layout_class`, `focus_starts_on_the_ring_and_a_press_on_chrome_does_not_move_it`, `only_the_focused_panel_carries_the_focused_border`, `tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in` |
| SURF-4 | Unproven, and unowned inside Phase 00. Nothing in the canonical journey blocks: the Attention queue exists so a background request does not open a modal, and a shelf overlays without blocking. The first blocking surface is a permission or confirmation prompt, which arrives with the phase that owns real tools |
| SURF-5 | `focus_returns_to_a_surface_that_comes_back` proves it for focus; scroll state arrives with the viewports in step 4 |
