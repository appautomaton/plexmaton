# Plan — Phase 04 stage 7, native math typesetting

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) |
| Contract | [Math research](../research/math-rendering.md), MD-1/MD-3/MD-4, SEL-2, FR-1–FR-4 |
| Status | Slice 3 of 4: geometry/height reuse delivered; owned work and transport pending |

## Outcome

Render mathematical structure as native text without compromising input responsiveness or exact
source ownership. Reuse a demonstrated TeX-math layout engine behind a first-party terminal
adapter. No browser screenshot, bitmap conversion or notification/A2A redesign. KaTeX font-level
fidelity on ordinary cells is not promised.

## Slices

1. **Rendering responsiveness — complete.** [FR-5](../specs/frame-loop.md) owns bounded
   pre-projection streaming coalescing; eight regression tests and the production-path rich-stream
   measurement prove source/revision preservation, immediate input, deadline/pressure behavior,
   failed output and three-width pointer/resize behavior. The pre-hit-test projection mutation
   fails the copy witness. Workspace tests, all-target check/Clippy, formatting and both offline
   PTY smokes pass. Re-rendered and inspected [wide](../../crates/plexmaton-tui/frames/markdown-style-120.svg),
   [medium](../../crates/plexmaton-tui/frames/markdown-style-88.svg) and
   [narrow](../../crates/plexmaton-tui/frames/markdown-style-60.svg) appearance is unchanged;
   static frames do not prove temporal smoothness. Cold rich-text layout, clipboard waits and saturated
   real-terminal latency remain follow-ups, not claimed improvements.
2. **Reusable layout and terminal adaptation — complete.**
   [MTH-1–MTH-4](../specs/math-layout.md) own the admitted RaTeX 0.1.14 parser/layout, native
   projection and evidence. All 61 supplied reply occurrences render at 120/88/60; 24 real Kitty
   pages preserve every native character in row order, with complete-formula pagination, redraw
   and clean exit. Three-width review frames and the derivative page were inspected. Eleven Rust
   regressions cover source, geometry, fonts/paint and refusals; a bare-body-copy mutation fails
   the delimiter witness. Nineteen Python/PTY checks include real Rust preparation. Workspace
   tests/check/Clippy, dependency and document gates pass. The old parser/structural layout and
   subexpression-copy APIs are removed; no TUI parser call or image formula transport was added.
   Complete typography acceptance, coarse root/delimiter joins, tmux and partial multicells remain
   production gates, not implied by character-state verification.
3. **Rendering cost, owned work and transport — in progress.** Shared borrowed wrap geometry and
   count-only literal measurement are delivered. Palette changes retain heights,
   anchors and pointer copy; six new regressions include a vector-reference property check and
   actual paragraph equivalence. Clearing geometry on a palette change fails the mutation witness.
   The three approved Markdown frames are unchanged after palette replacement; workspace gates and
   both PTY smokes pass. [FR-4](../specs/frame-loop.md) records roughly 6/7 ms plain-history cold/resize
   and the remaining roughly 30 ms cold-rich path. Next separate paint preparation from geometry
   completely, retaining restrained theme roles without per-color reflow. Prove cancellation, coalesced revisions, bounded cache, stale-result
   refusal and partial visibility before attaching expensive layout to the frame loop. Native
   OSC 66 shares one terminal-output owner with ordinary cells; capability limits remain visible.
   [Source-linked review](../spikes/kitty-text-sizing/README.md) establishes standalone engine-to-terminal
   flow, not production output ownership. Separate geometry invalidation from paint-only animation; clipboard delivery must
   not monopolize the interaction loop. General animation implementation remains the branding stage.
   Pure layout, job lifecycle, TUI revision adoption and terminal output have separate owners;
   extend concrete typed boundaries as actual consumers require them, without a general scheduler
   or renderer framework. Revised colors need rendered review before changing the UI/UX contract.
4. **Conversation integration and atomic copy.** Admit math syntax into Markdown with typed
   pending/error states and stable anchors. A formula click copies its complete original TeX,
   including the original opening and closing delimiters without normalizing them. A pointer
   range intersecting it expands to the whole delimited formula, while ordinary Markdown ranges
   retain plain-text copy and the message Copy action retains exact source. Preventing accidental
   partial selection is a mandatory acceptance criterion: even an edge intersection during a drag
   selects and highlights the entire formula, in either drag direction. Selection, highlighting
   and copied source must agree; snapping only the clipboard payload is insufficient. Rejected: bare-TeX
   formula copy; delimiters are part of the selected formula. Prove exact delimiter preservation,
   mixed ranges, streaming invalidation and clipped/reflowed selection, then review three-width
   rendered frames before updating the interaction contract. Run owning and workspace gates.
   Commit/merge only when requested.

## Order and why

The user prioritized daily-driver quality and performance before formula integration. The existing
render path already exposes redundant stream-driven frames, so the first slice is implementation
and regression evidence, not another exploratory spike. Reusable layout precedes its worker and
conversation integration. Delivered foundations are verified in independent commits; owned preparation and
conversation integration remain separately gated. Dependency types stay behind the owned adapter, never in the public formula API.

## Deliberately not in this plan

CAS/algebraic simplification, full TeX documents, arbitrary macros or file/network access, general
font/scheduler frameworks, image transports, new agent surfaces, notification changes, global FPS
configuration, or silent truncation.
