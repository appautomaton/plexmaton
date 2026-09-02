# Spec — Surface model

| Field | Value |
| --- | --- |
| Status | Implemented; SURF-2 and SURF-4 are unproven, having no caller yet |
| Owns | What a surface is, how one is registered, and which surface an event may reach |
| Depends on | The surface categories and routing rules in [`ui-ux.md`](../ui-ux.md) |
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
rather than restarting at the visible slice. **No surface needs this yet** — see Evidence.

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
| `id` | `SurfaceId`, a named variant. Never a number agreed by convention. Rejected: numeric identities, which break silently when a region is added; and a second layout computed for hit testing, whose failure is a click landing one panel over |
| `bounds` | The rectangle the surface occupies, whether or not all of it is visible |
| `clip` | The rectangle it is confined to. Not a field: nothing has needed a clip distinct from `bounds`, which is that rectangle |
| `z_index` | Draw and hit order among siblings. Zero for every tiled region; the second window as a shelf is one, floating inside the conversation, painted last and hit first, with the cells beneath it cleared before it paints |
| `kind` | What the surface *is*; every behavioural answer below is derived from it |
| `viewport` | How tall its content is and how far through it the user is. Filled in by the renderer, because measuring needs the text; `None` until a frame has drawn it |

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
| `Composer` | yes | yes | **yes** | no | no |
| `Inspector` | yes | yes | **yes** | no | **yes** |

`Inspector` joined in delivery step 7, named for what it is rather than for how it looks: this table
predicted `Shelf`, but a shelf is one of three presentations the same surface takes depending on
terminal size, and a kind named after one geometry would be the wrong name at the other two.
[`inspector`](./inspector.md) owns it.

`Modal` has not joined. Nothing blocks yet, so there is no surface to give the kind — the same
reason SURF-4 is unproven below. A kind with no surface using it is not added in advance.

### Hit testing

Among surfaces whose `kind` takes the pointer and whose `visible()` contains the point, the one with
the greatest `(z_index, id)` wins. A blocking surface truncates the search rather than being merely
topmost, so a hole in a modal is not a hole in its modality.

### Viewports

A surface's viewport is measured by the renderer, because how tall content is depends on the text
and the width it wraps to. Where the user put it lives in the projection and outlives the frame
(SURF-5); absence of a stored position is meaningful, and each surface then anchors to its own kind
of content — a conversation opens at its newest line, a list at its first.

Rejected: owning the wrapping. Roughly eighty lines of grapheme-and-width logic reaching the same
answer with our own bugs; `Paragraph::line_count` runs the same `WordWrapper` the renderer runs, and
the exact pin plus the committed lockfile make an unstable-API change a reviewed bump.

Eligibility for the wheel is whether a viewport *can move*, which
[`interaction-routing`](./interaction-routing.md) INV-3 turns into routing.

The conversation is the exception that earned its own contract: it is measured item by item, built
only where the viewport reaches, and parked against the message being read rather than a row
number. [`transcript-layout`](./transcript-layout.md) owns that.

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
| A terminal below the supported minimum | Nothing is registered, so a pointer event resolves to nothing rather than to a guess |
| Z-order exhausted by promotion | `SurfaceTreeError::ZOrderExhausted`; never a silent wrap that reorders the workspace |

## Out of scope

- **Terminal-event translation, capture, and the `Escape` ladder.**
  [`interaction-routing`](./interaction-routing.md) owns them.
- **Shelf geometry and the ten-row guarantee** (ui-ux §shelf). [`inspector`](./inspector.md) owns
  them.
- **Which surfaces exist.** That is layout's decision, and it changes with the layout class.

## Evidence

| Invariant | Proven by |
| --- | --- |
| SURF-1 | `every_registered_surface_is_drawn_inside_its_own_bounds`, `registered_surfaces_tile_the_terminal_without_gaps_or_overlap` |
| SURF-2 | Unproven; no surface overflows its parent yet. The base layer tiles the terminal, and the one surface above it, the second window as a shelf, lies wholly inside the conversation it covers. A transcript item scrolled past its viewport edge is content inside a surface, and which rows of it a frame builds is [transcript-layout](./transcript-layout.md) TR-2's business |
| SURF-3 | `the_inspector_takes_the_cursor_and_the_composer_keeps_one_row`, `chrome_is_neither_a_pointer_target_nor_a_focus_stop`, `the_focus_ring_wraps_in_both_directions`, `focus_outside_the_ring_enters_it_from_the_matching_end`, `the_focus_ring_loses_stops_without_ever_reordering`, `focus_starts_on_the_ring_and_a_press_on_chrome_does_not_move_it`, `only_the_focused_panel_carries_the_focused_border`, `tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in` |
| SURF-4 | Unproven; nothing blocks yet. The Attention queue exists so a background request does not open a modal, and a shelf overlays without blocking. The first blocking surface is a permission or confirmation prompt, which arrives with real tools |
| SURF-5 | `focus_returns_to_a_surface_that_comes_back` and `selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation` for focus; `an_untouched_panel_has_no_stored_position`, `a_resized_conversation_keeps_the_reader_on_the_same_message`, and `each_conversation_keeps_its_own_reading_position` for scroll |
