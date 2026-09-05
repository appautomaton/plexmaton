# Spec — Native math layout

| Field | Value |
| --- | --- |
| Status | Standalone engine/projection verified; no live conversation integration |
| Owns | Delimited formula source, engine admission and native text reservations |
| Depends on | MD-1/MD-3/MD-4, SEL-2, FR-2/FR-3 |
| Proven by | Rust corpus, source-linked Python/PTY checks and direct-Kitty review; worker and UI integration unproven |

## Invariants

**MTH-1 — Formula source is atomic.** A formula owns its complete original UTF-8 source, including
the original paired delimiters; every cell of its reserved rectangle, including blank and edge
cells, resolves to that whole source. Rejected: subexpression or bare-body copy, because an
accidental partial intersection must not produce partial TeX (SEL-2); UI drag snapping is unproven.

**MTH-2 — Native projection preserves mathematical meaning.** Positioned engine glyphs and rules
become disjoint cell reservations with admitted font mappings, script sizes and opaque/inherited
paint; unsupported output, collisions and indivisible width overflow are typed refusals. No
term-dropping, private-use glyph leakage, guessed negation or silent color substitution (MD-4).

**MTH-3 — Geometry has a retained origin.** A layout owns one immutable run list and origin shared
with its exact source owner; viewport slicing cannot restart it at a different baseline (MD-3).
Complete-formula review pagination is proven; arbitrary partial visibility and UI scrolling are
unproven until the transport/viewport slice.

**MTH-4 — Resource ownership precedes integration.** Admission bounds source bytes and expanded
nodes/depth, and projection bounds primitives, dimensions and cell allocation before painting.
Upstream CPU cancellation, aggregate retained memory and asynchronous revision/cache ownership
remain unproven; synchronous preparation must not enter the TUI event loop (FR-2).

## Evidence

| Invariant | Proof |
| --- | --- |
| MTH-1 | `formula_hit_cells_are_atomic_and_preserve_original_delimiters`, `source_and_native_limits_refuse_without_truncation_or_macro_leakage`; UI click, mixed-range dragging and highlight expansion unproven |
| MTH-2 | `complete_attention_reply_preserves_all_formula_occurrences_at_three_widths`, `structural_corpus_preserves_tables_roots_and_explicit_overflow`, `fraction_rows_and_paired_scripts_retain_engine_geometry`, `font_glyph_mapping_preserves_not_equal_double_struck_and_macron`, `explicit_colors_do_not_become_palette_inheritance`, `framed_paint_inherits_without_erasing_explicit_color`, `radicals_span_the_radicand_and_text_keeps_word_gaps`, `independent_native_overprint_is_refused` |
| MTH-3 | `formula_hit_cells_are_atomic_and_preserve_original_delimiters`, `complete_attention_reply_preserves_all_formula_occurrences_at_three_widths`; source-linked review pagination and terminal evidence below; live partial visibility/copy unproven |
| MTH-4 | `source_and_native_limits_refuse_without_truncation_or_macro_leakage`, `independent_native_overprint_is_refused`, `aggregate_cell_bound_is_checked_before_paint_allocation`; CPU isolation, cancellation and aggregate cache budget unproven |

## Model and limits

Original delimited source → RaTeX parser/admitted tree → RaTeX layout/display list → owned native
scene → monotone cell projection. The dependency boundary is private; no replacement TeX parser,
first-party fraction-layout engine or source reconstruction remains. The
[dependency audit](../standards/rust.md#audited-foundation) owns package admission.

`Formula::parse` accepts one complete `$…$`, `$$…$$`, `\(…\)` or `\[…\]` span. It is not a Markdown
recognizer. Native runs carry full, 0.7, 0.5 or two-row operator sizing, font treatment and paint;
palette inheritance does not require geometry changes. Admission normalizes absent frame paint
before upstream layout, keeping explicit black distinct. Only the verified private-use negation
overlay plus equals pair maps to `≠`; arbitrary paths, fonts, scales, background fills and other
unadmitted effects refuse explicitly.

The public constants own limits: 8 KiB original source, 4,096 expanded nodes, 4,096 engine
primitives, 512 per native dimension and 32,768 reserved cells. Upstream logical nesting is limited
to 32; post-parse admission also checks expanded depth 64. Those bounds do not prove interruptible
parsing or a low aggregate worker-memory budget. Native line wrapping is not implemented: the
unchanged wide structural fixture needs 91 columns and refuses at 88/60 rather than losing terms.

## Dependency admission

The exact 0.1.14 parser/layout/types/font pins resolve a five-crate RaTeX core including its lexer.
Published packages are MIT, edition 2021, without a declared MSRV; they build on workspace Rust
1.98.0. The [source](https://github.com/erweixin/RaTeX/tree/c902516816cdc84519827d8b46d1cd40270d0451)
was active on 2026-09-04. No browser, image backend, native library or font-file loader enters the
graph. Native output uses the terminal's fonts, not the source repository's separate font files.
New transitive thiserror 1 coexists with first-party 2; terminal foundations are unchanged.
Offline cargo-deny passed using the cached advisory database; feature-tree inspection and
cargo-machete passed. No claim of a freshly fetched advisory database follows from the offline gate.

## Rendered and terminal evidence

The [complete reply fixture](../../crates/plexmaton-math/fixtures/attention-derivatives.json) retains
61 verified byte ranges: 35 display and 26 inline occurrences. All prepare/project at 120/88/60.
The review example composes them with the original prose; Markdown markers remain visible because
it does not implement another Markdown renderer. Its page boundaries keep formulas whole.

Reviewed native-run projections at [120](../../crates/plexmaton-math/frames/native-120.svg),
[88](../../crates/plexmaton-math/frames/native-88.svg),
[60](../../crates/plexmaton-math/frames/native-60.svg) and
[derivatives](../../crates/plexmaton-math/frames/native-derivatives-88.svg) show the current boundary:
root joins and stretched delimiter pieces remain coarse. SVG is review evidence, never the
application's formula transport, and is not a pixel capture of Kitty.

On 2026-09-05, direct Kitty 0.46.1 / Menlo 15 consumed eight full-reply pages at each width, with
all 61 formula occurrences covered, explicit redraw and clean exit. The
[owned review transport](../spikes/kitty-text-sizing/README.md) sends native Unicode/OSC 66 from
the Rust export, not manually placed equation scenes. Nineteen Python/PTY checks include the real
Rust preparation path. Exact terminal glyph pixels, tmux, scrolling through partial multicells,
production output ownership and user acceptance of the complete typography remain unproven.

Reproduce from the worktree:

```console
cargo run --offline --locked -p plexmaton-math --example native_preview -- target/math-review
python3 .agents/spikes/kitty-text-sizing/launch_macos.py --page reply --reply-directory target/math-review
cargo run --release --offline --locked -p plexmaton-math --example corpus -- --measure
```

Release arm64 macOS, 2026-09-05: first cold batch prepares all 61 formulas in 3.914 ms and projects
183 layouts in 0.726 ms. Across 101 warm batches, preparation p50/p95/max is 1.595/2.079/2.385 ms;
projection is 0.350/0.512/0.582 ms, producing 3,063 native runs. These are standalone CPU-path
measurements, excluding terminal I/O, Markdown, worker coordination and input latency; adversarial
expansion, cancellation and aggregate memory remain separate gates.
