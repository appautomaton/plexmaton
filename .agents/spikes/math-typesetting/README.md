# Spike — Semantic math typesetting

Read for math display/source choices. Source comparison completed 2026-09-05.
[Track](../../research/math-rendering.md), [Markdown](../../specs/markdown.md).

## Finding

The implementation selects RaTeX's reusable parser/layout and a first-party native projection;
[MTH-1–MTH-4](../../specs/math-layout.md) own its boundary and evidence. The
[direct-Kitty follow-up](../kitty-text-sizing/README.md) verifies the complete source-linked reply
as **OSC 66 scaled text**. Ordinary cells cannot reproduce KaTeX typography; live worker,
viewport and selection integration remain open. The fixed comparison below explains why the
earlier cell renderer was not adopted.

KaTeX emits HTML/CSS and/or MathML, not terminal glyph commands. MathML loses exact TeX spelling;
OpenType MATH supplies font metrics/assemblies, not a parser.
[KaTeX](https://katex.org/docs/options), [MATH](https://learn.microsoft.com/en-us/typography/opentype/spec/math).
Ordinary Ratatui cells lack arbitrary sizing. OSC 66 adds scaled text; Kitty graphics placeholders,
Sixel/iTerm2 and raster-to-Braille remain images/sampled graphics requiring separate user acceptance.
[Image protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/#unicode-placeholders).

## Candidate evidence

| Candidate | Boundary, maintenance and admission result |
| --- | --- |
| `katex-rs` 0.3.0 | MIT, edition 2024, no declared MSRV, empty defaults; AST/source spans. Built without JS/C++; tracks KaTeX 0.18.5. Distinct from JS `katex` bindings. [Source](https://static.crates.io/crates/katex-rs/katex-rs-0.3.0.crate) |
| `tui-math` 0.1.1 | MIT, 2026-02-01, no MSRV; Ratatui 0.29/Crossterm 0.28. Repository unavailable. Reject defects below. [Package](https://docs.rs/crate/tui-math/0.1.1) |
| `mathtex` engine 0.1.2 | `f257708`, 2026-07-30: glyph/box/source-map IR; Rust font stack, unsafe translated TeX. MIT/Apache wrappers, separate core notices; formats/fonts required. Not run. [Source](https://github.com/gabriel-nsiqueira/mathtex) |
| Typst 0.15.1 | Apache-2.0, 2026-07-17; text/shape frames, different syntax. Not run/native-audited. [API](https://docs.rs/typst/0.15.1/typst/layout/enum.FrameItem.html) |
| RaTeX core 0.1.14 | Selected and audited in the isolated crate; parser plus reusable box layout and glyph/line/path output. All 61 supplied reply occurrences run through the native adapter; whole delimited source is owned separately. [API](https://docs.rs/ratex-types/0.1.14/ratex_types/display_item/enum.DisplayItem.html). No PNG wrapper or font-file renderer adopted |

## Native scaled text: OSC 66 (protocol analysis)

UTF-8 text, not images. `s=1..7` reserves `s*w` columns × `s` rows;
`n/d` shrinks glyphs without shrinking that reservation. `v=0/1/2` aligns top/bottom/center;
`h` similarly aligns horizontally. `n=1:d=2` gives superscripts, adding `v=1` gives subscripts;
`w=1` can pack two half-sized letters. Width overruns may truncate or shrink unpredictably.
No arbitrary baseline offset or font/glyph-ID selector.

Detection: CR, CPR, `OSC 66;w=2;space`, CPR, `OSC 66;s=2;space`, CPR. Two successive
two-column advances distinguish width and scaling support. Probe only in an owned blank region,
with bounded response handling and redraw. Overwriting any top-row cell erases the whole block;
writing into lower rows skips past it. Erase controls erase intersecting blocks; line edits can
erase split blocks. Oversized blocks may disappear on resize. No viewport crop command exists.
[Protocol](https://sw.kovidgoyal.net/kitty/text-sizing-protocol/).

| Environment | Primary-source status |
| --- | --- |
| Kitty | Scaling since 0.40.0 (2025-03-08). [Changelog](https://sw.kovidgoyal.net/kitty/changelog/) |
| Foot | 1.21.0 documents width only. [Changelog](https://codeberg.org/dnkl/foot/src/branch/master/CHANGELOG.md) |
| Ghostty | OSC parser implemented; rendering issue still open. [Issue 10333](https://github.com/ghostty-org/ghostty/issues/10333) |
| tmux | Current `input_exit_osc` has no 66 case; unknown OSC ignored. Passthrough writes raw bytes without teaching its grid scaled cells, so is not a supported redraw path. [Source](https://github.com/tmux/tmux/blob/master/input.c) |

Kitty tests retain multicells split across history/screen, not arbitrary application subviewports.
Ratatui diffs can erase/skip their regions. Own disjoint rectangles and clearing/repainting;
start with `s=1` fractional scripts. [Tests](https://github.com/kovidgoyal/kitty/blob/master/kitty_tests/multicell.py).

Corpus proposal: arbitrary script letters, but sup/sub cannot independently overprint one cell;
use separate rows. Fractions/roots/matrices/cases still need bars/delimiters; scaled parentheses
also widen. Alignment stays structural; packing cannot guarantee width fit; incomplete input
still needs pending state. Compare cells/OSC 66 at 120/88/60, overwrite and partial scroll.

## Executed fixed corpus

[corpus.rs](./corpus.rs) fixes eight inputs. A = `tui-math`/`latex2mathml` 0.2.3; B = Rust
`katex-rs` 0.3.0 display MathML into the **same** cell renderer. B accepts seven complete inputs;
this proves a non-image path, not general KaTeX compatibility or correct layout.

| Input | Observed A | Observed B |
| --- | --- | --- |
| Nested fraction | 6×5, baseline 1; stacked hierarchy | Same cells |
| Nested root | 14×3; underscore roof and displaced radical stem | Same cells |
| `x_{ij}^{n+1}+\alpha_2` | Incorrect: `ijⁿ⁺¹` sits below `x` | One-row `xᵢⱼⁿ ⁺ ¹ + α₂`; incomplete script typography |
| 2×2 `pmatrix` | 8×2; parentheses enclose bottom row only | Same defect |
| `cases` | Unknown environment error | Two branches; brace only on bottom row |
| `aligned` | Unknown environment error | Two rows with aligned equals signs |
| Wide sum/fraction | 113×3 | Same; fits 120, exceeds 88 by 25 and 60 by 53 cells |
| `\frac{1}{1+\sqrt{` | Parse error | Parse error; no invented completion |

Witnesses (A), not approved appearance:

```text
  1                a  b
──────           ( c  d )
     a
1 +  ─
     b
```

Rows 2..4 remain `     a` / `1 +  ─`: geometry access, not TUI scrolling. 120/88/60 are overflow
checks, **not** reviewed frames. No selection/cancellation/cache tests. One release run, macOS
arm64, rustc 1.98.0: A complete calls 18–232 µs, one sample each, fresh uncached renderer;
B not timed. No latency guarantee.

Reproduce from repository root, outside the main Cargo graph:

```sh
math_spike_dir=$(mktemp -d /tmp/plexmaton-math-typesetting.XXXXXX)
cp .agents/spikes/math-typesetting/{Cargo.toml,corpus.rs} "$math_spike_dir/"
CARGO_HOME="$math_spike_dir/cargo-home" CARGO_TARGET_DIR="$math_spike_dir/target" \
  cargo run --release --manifest-path "$math_spike_dir/Cargo.toml"
```

Direct versions pinned; transitives resolve anew. Measured lockfile:
`/tmp/plexmaton-math-typesetting.jtGrdu`. Crates API 403; `tui-math` clone failed.

## Product acceptance (unproven)

The [stage plan](../../plans/phase-04-stage-07-math-typesetting.md) owns worker, cache, transport and
conversation integration. The native crate reuses engine geometry instead of owning another TeX
layout implementation. Source limits and exact whole-formula ownership are proven there;
interruptible CPU work and aggregate resource ownership are not.

Visual gate interactions:

- Stable streaming: last valid preview marked “updating”, atomic replacement at the same anchor.
  Rejected: silent brace insertion, which asserts unwritten math.
- Source inspection: formula click and intersecting drags select the whole original delimited
  formula, with a keyboard path. Rejected: subexpression selection and copy reconstructed from art.
- Derivation reading: aligned relations/branches, structural wrapping or explicit overflow and
  focused horizontal view. Rejected: shrinking/clipping terms; focus/scroll ownership needs review.

Without proven scaling, the reply review shows source. Production capability/fallback behavior
must be explicit and tested. Real probe/native text evidence belongs to the direct-Kitty follow-up;
this source-comparison corpus does not prove terminal behavior.
