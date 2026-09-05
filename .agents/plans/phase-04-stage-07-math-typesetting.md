# Plan — Phase 04 stage 7, native math typesetting

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) |
| Contract | [Math research](../research/math-rendering.md), MD-1/MD-3/MD-4, SEL-2, FR-1–FR-5 |
| Status | Slice 2 of 4: native math foundation pending; rendering performance delivered |

## Outcome

Native mathematical presentation with exact source ownership and responsive interaction.
Keep pure layout, task lifecycle, TUI adoption and terminal output separately testable.
The approved pastel Markdown appearance stays unchanged; new formula colors require rendered review.

## Slices

1. **Rendering responsiveness — complete.** [FR-5](../specs/frame-loop.md) owns bounded stream
   coalescing; [FR-4](../specs/frame-loop.md) and [TR-1](../specs/transcript-layout.md) own shared
   wrap geometry, count-only literal measurement and palette-independent heights. Failure,
   pressure, input, exact-copy and three-width palette regressions pass. The unchanged
   [wide](../../crates/plexmaton-tui/frames/markdown-style-120.svg),
   [medium](../../crates/plexmaton-tui/frames/markdown-style-88.svg) and
   [narrow](../../crates/plexmaton-tui/frames/markdown-style-60.svg) frames were inspected.
   The measured cold-rich path, clipboard waits and saturated terminal latency remain open.
2. **Reusable math layout — pending.** Admit the tested TeX parser/layout behind a private
   boundary, retain complete original delimited source, and verify native glyph/rule projection
   at three widths. Unsupported output and width overflow must be explicit; no live TUI parser
   call or formula image transport.
3. **Owned work and transport — pending.** Build on the delivered geometry/height reuse.
   Separate paint preparation from geometry completely, then prove cancellation, coalesced
   revisions, bounded caches and stale-result refusal. One output owner must coordinate ordinary
   cells and scaled native text, including partial visibility and terminal capability limits.
   Clipboard delivery must not monopolize the interaction loop. General animation is the branding
   stage, not a reason to add a global frame clock.
4. **Conversation integration and atomic copy — pending.** Add typed pending/error states and
   stable formula anchors to Markdown. Click and either-direction drags, including an edge
   intersection, select and highlight the complete original formula; copying includes its original
   opening and closing delimiters. Ordinary ranges retain plain-text copy and message Copy retains
   exact Markdown source. Prove mixed ranges, streaming invalidation, reflow and clipping, then
   review three-width frames before changing the interaction contract.

## Order and why

The user prioritized daily-driver performance before formula integration. The reusable engine
precedes its owned preparation and conversation integration. Each delivered boundary carries its
own tests; complete formula availability is not implied by landing its foundation.

## Deliberately not in this plan

CAS, full TeX documents, arbitrary macro/file/network access, image transports, general rendering
or scheduler frameworks, A2A/notification changes, global FPS configuration or silent truncation.
