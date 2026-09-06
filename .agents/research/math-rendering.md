# Track — Readability-first math rendering

| Field | Value |
| --- | --- |
| Status | Native RaTeX conversation path approved; remaining comparison covers portability and source reveal |
| Kind | Research track, not a delivery phase |
| Parent roadmap | [Plexmaton Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Entry condition | Met: transcript clipping, revisioned layout and semantic copy exist |
| Blocks | Broader terminal support and non-reflowing source inspection |

## Question

How can formulas feel like readable, selectable parts of a working conversation, with the quality
the user associates with KaTeX, without a browser screenshot or an opaque equation-image widget?
The [math-typesetting spike](../spikes/math-typesetting/README.md) owns source comparisons and
candidate results. The project adopts RaTeX parser/layout behind
[MTH-1–MTH-5](../specs/math-layout.md), including live full-reply native rendering, owned
preparation/output and atomic selection. The user approved the live direct-Kitty appearance on
2026-09-06; this remaining track does not reopen the selected parser/layout engine.
The [direct-Kitty spike](../spikes/kitty-text-sizing/README.md) owns real scaled-text observations
and the separate ML fixture corpus. Its script typography direction was accepted by the user.

The user rejects a KaTeX-to-image approach and accepts maintaining a focused native renderer.
Investigate genuine cell typesetting first. A renderer-neutral glyph/box engine with an optional
terminal graphics backend is a separate conditional proposal requiring agreement: raster pixels
remain images even when their placement is cell-aligned or uses Unicode placeholders.

Rejected: prescribing a raster cache, a pixel transport and a Braille/half-block transport before
proving they match the user's request. Sampling the same bitmap into cell glyphs is not native
mathematical layout.

## Constraints and unproven goals

- Preserve one exact semantic source. Copy never reconstructs LaTeX from displayed cells or pixels.
- Mathematical structure must survive presentation: numerator/denominator, roots, scripts, matrix
  rows and alignment are not optional decoration. A plausible-looking but wrong formula is a failure.
- KaTeX-quality typography is a goal, **unproven** in ordinary terminal character cells. Report
  font metrics, stretched glyphs and subcell placement limits before proposing any guarantee; a
  product-contract change requires the user's decision, not a research shortcut.
- Source reveal and exact copy must be available without shifting surrounding content, including
  a keyboard path. Formula hits are atomic over the complete original delimited source; application
  drag expansion/highlighting and mixed Markdown copy are proven by MTH-1. Non-reflowing source
  reveal with a keyboard path remains unproven; whole-entry keyboard source copy exists.
- Layout participates in owned viewport clipping and scrolling. Partial visibility retains the
  formula's logical origin. Terminal resize must not cause invisible terms to be silently dropped.
- Parsing/layout work needs a bounded asynchronous owner, cancellation/revision handling and
  cache invalidation covering source, width, renderer version and relevant font/display metrics.
  Paint-only changes must not rebuild geometry; old work cannot replace a newer stream revision.
- Unsupported syntax, incomplete streams, resource exhaustion and real failures are explicit
  outcomes. Do not pretend a render error is a successful simplified equation.
- Notification layout, A2A geometry and the approved Markdown palette are unchanged by this track.

## Comparison corpus

Freeze one corpus before a candidate run: fractions/nested fractions, roots, paired super/subscripts,
matrices, cases/aligned, a formula wider than the viewport, Unicode symbols and incomplete streamed
input. Preserve failures beside successes. A repaired candidate runs the same corpus; do not change
the input to make it appear supported.

Measure or explicitly leave unmeasured:

- Structural correctness and readability at several widths/heights, not just successful parsing.
- Parse/layout latency, cold initialization, cache size and dependency/build footprint.
- Streaming behavior, cancellation and output/expansion bounds.
- Clipping, partial scrolling, width overflow and exact-source copy.
- Terminal/font portability, SSH and tmux; distinguish source inspection from real terminal tests.

## Candidate boundaries

Keep four decisions separate: TeX/LaTeX recognition; formula structure with source spans; math box
layout; terminal projection. A Rust API may wrap JavaScript/C, and native Rust HTML/MathML output
still needs a terminal layout engine. Review the actual output and ownership boundary, not a crate's
name or an image in its README.

The selected reuse boundary and fixed-corpus evidence are in MTH-1–MTH-5. Remaining decisions concern
portable typesetting when native sizing is unavailable, true partial-multicell display and source reveal;
no external browser, renderer service, global install or real model request is required.

## Exit condition

- A feasible next implementation slice is identified, with honest fidelity and syntax limits.
- The user has seen representative output and chosen any typography-versus-transport tradeoff.
- A candidate is selected only after the fixed corpus, exact copy, resource ownership and clipping
  behavior are demonstrated; until then those requirements are marked unproven.
- Promote only durable findings into a mechanism spec when implementation starts. Keep the
  comparison here until decided; no production-readiness claim follows from a research report.
