# Spec — Surface model

| Field | Value |
| --- | --- |
| Status | Implemented; SURF-2 remains unproven, with no generic clipping caller yet |
| Owns | What a surface is, how one is registered, and which surface an event may reach |
| Depends on | The surface categories and routing rules in [`ui-ux.md`](../ui-ux.md) §surface model and §input and event-routing contract |
| Proven by | `plexmaton-tui::layout`, `::surface`, and `::render` tests |

## Invariants

**SURF-1 — Drawn equals registered.** Layout is the only producer of workspace rectangles, and every
rectangle it computes is registered in the `SurfaceTree` the router borrows and never mutates. The
renderer draws each surface only inside that surface's visible rectangle, and measures its frame
rows and columns independently from the edges it paints.

**SURF-2 — Clip, not bounds.** A surface carries the rectangle it occupies and the rectangle it is
clipped to; hit testing and painting use their intersection, so a surface scrolled or covered past
its parent's edge is not hit where it is not visible, and its content keeps its logical origin.
Unproven: no surface needs it yet, and there is no clip field until one does.

**SURF-3 — Focus is a surface property.** At most one surface holds keyboard focus, and it is one
the focus ring contains. Whether a text cursor exists is derived from the focused surface's kind,
never asserted independently.

**SURF-4 — A modal blocks delivery below it.** While a surface that blocks is registered, pointer
hit testing stops at it and the focus ring contains only it. The decision region is opened by a
user action from Attention, or by the primary agent's own approval arriving in the conversation the
user is already in (ATT-1); a request from a *background* agent alone registers no modal. It takes
rows of its own between that conversation and its composer rather than the composer's rectangle:
answering a tool call and typing the next instruction are two inputs, and the second is not the
place to put the first.

The configuration page uses `Modal` above approvals; `CommandPalette` is a blocking text input
above both. Their dismissal and compact geometry are INV-12 and INV-13.

**SURF-5 — Hidden state survives.** A surface's focus and scroll state belong to the surface, not
to the frame that drew it. Covering, unregistering for a frame, or re-registering does not reset
them, so reopening restores what the user left.

## Model

```text
layout::workspace(area, …) ─▶ SurfaceTree ─▶ render draws each surface into visible()
                                   │
                                   └─▶ RouterContext borrows it for hit testing
```

| Field | Meaning |
| --- | --- |
| `id` | `SurfaceId`, a named variant. Rejected: numeric identities, which break silently when a region is added; and a second layout computed for hit testing, whose failure is a click landing one panel over |
| `bounds` | The rectangle the surface occupies. `visible()` is `bounds ∩ clip` (SURF-2), and until a surface exists that does not fit its parent, `clip` is `bounds` and not a field |
| `z_index` | Draw and hit order among siblings: zero for tiled regions, one for the shelf, ten for approval, fifteen for configuration, twenty for the command palette |
| `kind` | What the surface is; every behavioural answer below is derived from it |
| `viewport` | Content height and the user's position through it, filled in by the renderer because measuring needs the text; `None` until a frame has drawn it |

| Kind | Pointer | Focusable | Cursor | Blocks below | Dismissible |
| --- | --- | --- | --- | --- | --- |
| `Panel` | yes | yes | no | no | no |
| `Chrome` | no | no | no | no | no |
| `Composer` | yes | yes | yes | no | no |
| `Inspector` | yes | yes | yes | no | yes |
| `Modal` | yes | yes | no | yes | yes |
| `CommandPalette` | yes | yes | yes | yes | yes |

Deriving the five answers from `kind` makes the boolean combinations that mean nothing, a status
line holding the cursor, or a modal that does not block, unrepresentable.

**Hit testing.** Among surfaces whose kind takes the pointer and whose `visible()` contains the
point, the greatest `(z_index, id)` wins. A blocking surface truncates the search rather than being
merely topmost, so a hole in a modal is not a hole in its modality.

### Viewports

The renderer measures a viewport, because content height depends on the text and the wrapping
width. Where the user is lives in the projection and outlives the frame (SURF-5), and an absent
position is meaningful: a conversation opens at its newest line, a list at its first. Eligibility
for the wheel is whether the viewport can move (INV-3). The conversation is measured item by item
and parked against the message being read; [transcript-layout](./transcript-layout.md) owns that.
Rejected: owning the wrapping, roughly eighty lines of grapheme-and-width logic reaching the same
answer with our own bugs; `Paragraph::line_count` runs the `WordWrapper` the renderer runs, and the
exact pin plus the committed lockfile make an unstable-API change a reviewed bump.

**Focus.** The focus ring is the registered focusable surfaces in a deterministic order, so
keyboard-only navigation is inspectable (ui-ux §user control). A press focuses the surface it hits;
hover never does (INV-3).

## Failure modes

| Situation | Response |
| --- | --- |
| A second surface registered under one identity | `SurfaceTreeError::DuplicateSurface`; never a silent overwrite |
| A surface whose `visible()` is empty | Registered, painted as nothing, unhittable; it exists so its retained state survives (SURF-5) |
| Focus held by a surface no longer registered | Focus moves to the first ring stop; never a dangling identity |
| A press on a surface that is not focusable | The press routes, focus does not move |
| A terminal below the supported minimum | Nothing is registered, so a pointer event resolves to nothing rather than a guess |
| Z-order exhausted by promotion | `SurfaceTreeError::ZOrderExhausted`; never a silent wrap that reorders the workspace. Unreachable while one surface floats |

## Evidence

| Invariant | Proven by |
| --- | --- |
| SURF-1 | `every_registered_surface_is_drawn_inside_its_own_bounds`, `registered_surfaces_tile_the_terminal_without_gaps_or_overlap`, `a_partial_frame_measures_each_axis_from_the_edges_it_paints` |
| SURF-2 | Unproven; no surface overflows its parent. The inspector is one composite surface whose entered input leaves a smaller conversation rectangle, but both parts remain inside its bounds and one local accessor keeps painting and row hit resolution together; a generic clip field waits for a surface that needs clipping. Which rows of a scrolled item a frame builds is [transcript-layout](./transcript-layout.md) TR-2's business |
| SURF-3 | `the_inspector_takes_the_cursor_and_the_composer_keeps_one_row`, `chrome_is_neither_a_pointer_target_nor_a_focus_stop`, `focus_starts_on_the_ring_and_a_press_on_chrome_does_not_move_it`, `the_focus_ring_wraps_in_both_directions`, `focus_outside_the_ring_enters_it_from_the_matching_end`, `the_focus_ring_loses_stops_without_ever_reordering`, `only_the_focused_panel_carries_the_focused_border`, `tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in` |
| SURF-4 | `a_blocking_surface_prevents_delivery_below_it`, `approval_keys_stay_inside_the_blocking_surface`, `an_open_approval_blocks_the_workspace_and_returns_only_the_selected_decision`, `the_approval_frames_match_their_fixtures` |
| SURF-5 | `focus_returns_to_a_surface_that_comes_back` and `selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation` for focus; `an_untouched_panel_has_no_stored_position`, `a_resized_conversation_keeps_the_reader_on_the_same_message`, and `each_conversation_keeps_its_own_reading_position` for scroll |
