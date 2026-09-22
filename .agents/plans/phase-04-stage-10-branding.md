# Plan — Phase 04 stage 10, branding

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) stage 10 |
| Contract | [ui-ux](../ui-ux.md) "An empty conversation shows the mark"; MOT-1 to MOT-3 in [stage 34's plan](./phase-04-stage-34-conversation-chrome.md) until promoted; FR-1 in [frame-loop](../specs/frame-loop.md) |
| Evidence | [Conversation chrome spike](../spikes/conversation-chrome/README.md): mark proportions, motion choreographies, the Nerd Font shape family and the measured cells |
| Status | Slice 0 of 4, unstarted; begins after stage 34's slice 3 lands the motion owner |

## Outcome

The mark is a rounded-square frame around a circular centre. In the terminal it is drawn in cells
from box drawing and one glyph of the Material Design set the Nerd Font already supplies, at an odd
size chosen from the terminal's measured cell so it comes out square. It greets an empty
conversation and leaves with the first message. In motion the frame breathes between weights and
drifts between palette slots while the centre morphs through circle, ring, rounded square and
square. It is presentation and nothing else reads it.

## Slices

1. **Geometry.** A `mark` module draws the frame at three weights from light rounded, heavy and
   half-block box drawing, and the centre from the glyph family recorded in the spike, always on odd
   axes so the centre has one cell. The width for a given row count is the odd count nearest square
   for the cell the terminal reports in pixels; when it reports none, 7×3 and 11×5. Static previews
   first: frames at wide, medium and narrow, reviewed by the user before any motion.
2. **Placement.** Centred in an empty conversation with the product's name beneath; gone with the
   first message; nothing else in the frame moves for it. Closes with the empty-state frames at three
   widths and the existing canonical frames unchanged.
3. **Motion.** Weight breathing, colour drifting blue, purple, magenta, purple, blue, cyan, and the
   centre's morph, all on MOT-1, all presentation (MOT-3); MOT-2 for every glyph the centre uses.
4. **Outside the terminal.** An SVG of the same geometry for the README; documents current; this
   plan deleted; the phase row says done.

## Order and why

Static geometry before motion, as the stage has always said. Placement before motion so the frames
that motion must not disturb exist first. The motion owner is stage 34's, so this stage waits for it
rather than growing a second clock.

## Deliberately not in this plan

A bitmap or native-protocol logo; a splash that delays input; a fixed cell size; a mark anywhere
but the empty conversation until the user asks for another place.
