# Track — Readability-First Math Rendering

| Field | Value |
| --- | --- |
| Status | Not started |
| Kind | Research track, not a delivery phase |
| Parent roadmap | [Plexmaton Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Entry condition | Met; see below |
| Blocks | Productionizing math in Phase 04 |

## Why this is a track and not part of a phase

It shares almost no machinery with the interaction spine, which needs focus, hit testing, pointer
capture, and viewports, while this track needs a layout engine, a raster cache, and two display
transports. One exit gate for both would couple a bounded phase to an open-ended engine
comparison.

Separating them does not soften the product invariants, which live in the parent roadmap and
remain locked: typeset math is the primary presentation, source is preserved behind it, and
a terminal without an image protocol gets Unicode cell graphics derived from the same rendered
result rather than raw LaTeX.

## Entry condition

Met. The interaction spine provides:

- A viewport that clips content to a rectangle and owns its own scroll offset.
- A transcript block whose layout is invalidated by revision and width.
- Semantic copy that returns source rather than rendered cells.

Without these, a math prototype cannot demonstrate clipping, partial scrolling, or source copy,
which are the properties that actually decide the engine.

## Comparison corpus

One bounded corpus, fixed before any engine is installed, containing:

- Fractions and nested fractions
- Roots and nested roots
- Matrices
- `cases`
- `aligned`
- An equation wider than the viewport
- Unicode symbols outside ASCII
- An incomplete expression arriving mid-stream

The corpus is committed as fixture data. Adding cases to make a candidate look better after the
comparison begins invalidates the comparison.

## Candidates

| Candidate | Note |
| --- | --- |
| Actual KaTeX output converted to a renderer-neutral or raster form | May require a JavaScript or external rendering boundary; define the input/output and cache contract before embedding any runtime |
| Native Rust RaTeX/KaTeX-compatible layout | `ratatex 0.1.0` currently depends on Ratatui 0.29 and an older `ratatui-image`; evaluate in an isolated spike or port it. Duplicate Ratatui generations must never enter the application |
| Renderer-neutral TeX layout such as `mathtex` | `mathtex 0.1.x` is early; evaluate against the corpus before it becomes a dependency |

Transport candidates, kept behind adapters and out of the main graph until one is selected:

| Crate | Role | Constraint |
| --- | --- | --- |
| `ratatui-image 11.0.6` | Pixel transport | Compatible with Ratatui 0.30; use `default-features = false` so dynamic Chafa and broad image formats do not enter accidentally |
| `image 0.25.10` | Raster buffer | Use `default-features = false`; enable only the formats the spike actually needs |

Chafa, Sixel encoders, Kitty helpers, and SVG rasterizers are transport implementation details.
Prefer the smallest chain that proves both pixel and no-image output.

## Decision criteria

Evaluate every candidate on the same corpus, against both a graphics-protocol transport and a
Unicode cell-graphics transport:

- Readability of the typeset result at realistic terminal sizes
- Supported syntax, and which corpus entries fail
- Render latency and cache size
- Behavior under clipping and partial scrolling, including retaining the logical image origin
- Source reveal and exact-source copy
- tmux and SSH behavior
- Failure representation when parsing genuinely fails

Screenshots alone do not select an engine.

## Exit condition

- One engine and two transports selected, with the comparison recorded.
- Unsupported corpus entries listed explicitly rather than quietly dropped.
- Cache key defined over source, display width, theme, scale, renderer version, and transport.
- Source reveal and copy proven for both transports.
- Findings that change a durable invariant promoted to the parent roadmap.
