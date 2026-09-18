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
hit testing stops at it and the focus ring contains only it. Main-agent approvals use a non-blocking
`Panel` inside their conversation (ATT-1): their rows remain above the composer, and Esc returns
focus to input without removing the card. User-opened background request overlays use `Modal`; a
background request arriving alone registers no modal. User-opened `CommandInspection` is a read-only
modal above the approval (APD-2). Narrow Agents is a full-body `Modal` above every
conversation-owned popup and decision; the conversation tree and Drawer remain workspace overlays
above it. The Drawer is a blocking text input, or a `Modal` while one of its navigated pages is
open; its dismissal and geometry are DRW-2 and DRW-3.

**SURF-5 — Hidden state survives.** A surface's focus and scroll state belong to the surface, not
to the frame that drew it. Covering, unregistering for a frame, or re-registering does not reset
them, so reopening restores what the user left.

## Model

Product vocabulary is defined in ui-ux §Product vocabulary. The primary conversation surface uses
`SurfaceId::Transcript`; `Inspector` is a composite containing a sub-agent conversation and its
steering input. `SurfaceId::Agents` is the roster, never another name for a transcript.

```text
layout::workspace(area, …) ─▶ SurfaceTree ─▶ render draws each surface into visible()
                                   │
                                   └─▶ RouterContext borrows it for hit testing
```

| Field | Meaning |
| --- | --- |
| `id` | `SurfaceId`, a named variant. Rejected: numeric identities, which break silently when a region is added; and a second layout computed for hit testing, whose failure is a click landing one panel over |
| `bounds` | The rectangle the surface occupies. `visible()` is `bounds ∩ clip` (SURF-2), and until a surface exists that does not fit its parent, `clip` is `bounds` and not a field |
| `z_index` | Draw and hit order among siblings: zero for tiled regions, one for the Inspector shelf, ten for approval, fourteen for Narrow Agents, fifteen for the conversation tree, twenty for the Drawer |
| `kind` | What the surface is; every behavioural answer below is derived from it |
| `viewport` | Content height and the user's position through it, filled in by the renderer because measuring needs the text; `None` until a frame has drawn it |

| Kind | Pointer | Focusable | Cursor | Blocks below | Dismissible |
| --- | --- | --- | --- | --- | --- |
| `Panel` | yes | yes | no | no | no |
| `Chrome` | no | no | no | no | no |
| `Composer` | yes | yes | yes | no | no |
| `Inspector` | yes | yes | yes | no | yes |
| `Modal` | yes | yes | no | yes | yes |
| `Drawer` | yes | yes | yes | yes | yes |

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

[Named proofs](../evidence/surface-model.md), one row an invariant.
