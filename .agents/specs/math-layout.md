# Spec — Native math layout

| Field | Value |
| --- | --- |
| Status | Live native conversation math implemented; direct-Kitty appearance approved 2026-09-06; portability and source reveal remain unproven |
| Owns | Delimited formula source, engine admission, native reservations and terminal composition |
| Depends on | MD-1/MD-3/MD-4, SEL-2, FR-2/FR-3 |
| Proven by | Rust corpus, real preparation child, atomic selection and output tests, three-width workspace frames and owned direct-Kitty CLI check |

## Invariants

**MTH-1 — Formula source is atomic.** A formula owns its complete original UTF-8 source, including
the original paired delimiters; every cell of its reserved rectangle, including blank and edge
cells, resolves to that whole source. Rejected: subexpression or bare-body copy, because an
accidental partial intersection must not produce partial TeX (SEL-2). Clicking copies and highlights
that source; either drag direction expands every intersection, including a blank edge cell, to the
whole formula. Streaming completion or Markdown reinterpretation invalidates an obsolete atom.

**MTH-2 — Native projection preserves mathematical meaning.** Positioned engine glyphs and rules
become disjoint cell reservations with admitted font mappings, script sizes and opaque/inherited
paint; unsupported output, collisions and indivisible width overflow are typed refusals. No
term-dropping, private-use glyph leakage, guessed negation or silent color substitution (MD-4).

**MTH-3 — Geometry has a retained origin.** A layout owns one immutable run list and origin shared
with its exact source owner; viewport slicing cannot restart it at a different baseline (MD-3).
Only complete, visible, unoccluded native runs reach terminal output. A bisected multicell retains
its origin and atomic source range, with a visible `⋮` and `Math clipped` notice; it never restarts
at the viewport edge or overwrites another surface.

**MTH-4 — Resource ownership precedes integration.** Admission bounds source bytes and expanded
nodes/depth, and projection bounds primitives, dimensions and cell allocation before painting.
The PRE-2 child owns synchronous parsing/layout and terminates on cancellation; immutable native
runs enter PRE-1's validated reply and MD-4's accounted cache. Synchronous preparation never enters
draw, hit testing or the TUI input loop (FR-2); byte limits are not an OS memory guarantee.

**MTH-5 — Native text and cells commit one frame.** One CLI output owner serializes cell diffs,
native text and clipboard effects; native reservations and hit maps commit only after the complete
frame succeeds (PRE-3). Startup cursor measurements must prove width and scaling before native
output; unsupported, unverified and multiplexer cases visibly retain source instead of assuming
capability from a terminal name.

## Evidence

| Invariant | Proof |
| --- | --- |
| MTH-1 | `formula_hit_cells_are_atomic_and_preserve_original_delimiters`, `formula_clicks_and_reverse_edge_drags_select_highlight_and_copy_the_complete_source`, `an_atomic_range_highlights_every_blank_and_edge_cell`, `formula_source_fallback_and_reflow_preserve_atomic_selection_without_repreparing_for_paint`, `streamed_formula_completion_and_markdown_reinterpretation_cannot_leave_partial_tex_selected`, `real_preparation_worker_preserves_the_complete_native_math_reply` |
| MTH-2 | `complete_attention_reply_preserves_all_formula_occurrences_at_three_widths`, `structural_corpus_preserves_tables_roots_and_explicit_overflow`, `fraction_rows_and_paired_scripts_retain_engine_geometry`, `font_glyph_mapping_preserves_not_equal_double_struck_and_macron`, `explicit_colors_do_not_become_palette_inheritance`, `framed_paint_inherits_without_erasing_explicit_color`, `radicals_span_the_radicand_and_text_keeps_word_gaps`, `independent_native_overprint_is_refused` |
| MTH-3 | `native_runs_keep_their_origin_and_never_cross_viewport_or_overlay_edges`, `native_table_cells_keep_atomic_geometry_and_exact_tabular_copy_when_narrow`, `complete_reply_composes_native_math_and_exact_atomic_maps_at_three_widths`; real partial-multicell pixel fidelity remains unproven |
| MTH-4 | `source_and_native_limits_refuse_without_truncation_or_macro_leakage`, `aggregate_cell_bound_is_checked_before_paint_allocation`, `native_reply_roundtrip_validates_the_complete_corpus_and_rejects_forged_runs`, `native_transport_limits_refuse_locally_before_a_prepared_reply_is_encoded`, `preparation_wire_rejects_mismatched_math_capability_geometry_and_atomic_maps`, `replacing_projection_revokes_math_work_even_when_semantic_keys_are_identical`; PRE-2 owns process timeout/replacement/shutdown evidence |
| MTH-5 | `streaming_preparation_preserves_native_runs_without_rewriting_them`, `cells_native_math_and_clipboard_share_one_ordered_output_owner`, `native_encoder_rejects_invalid_scale_controls_and_capacity_before_output`, `native_capability_requires_the_complete_measured_cursor_sequence`, `failed_native_output_keeps_the_last_painted_hit_map_and_frame_identity`, `formula_source_fallback_and_reflow_preserve_atomic_selection_without_repreparing_for_paint`; direct-Kitty CLI evidence below |

## Model and limits

Original delimited source → RaTeX parser/admitted tree → RaTeX layout/display list → owned native
scene → monotone cell projection. The dependency boundary is private; no replacement TeX parser,
first-party fraction-layout engine or source reconstruction remains. The
[dependency audit](../standards/rust.md#audited-foundation) owns package admission.

`Formula::parse` accepts one complete `$…$`, `$$…$$`, `\(…\)` or `\[…\]` span. It is not a Markdown
recognizer: MD-1 owns syntax recognition outside code, HTML and link/image literals. Incomplete
backslash spans and individual formula refusals retain their original source with a local label.
Dollar spans follow CommonMark's math-extension grammar. Inline boxes share the prose axis;
display boxes occupy their own band. Table cells retain the same atomic ranges when a narrow
grid becomes labelled values. Native runs carry full, 0.7, 0.5 or two-row operator sizing, font treatment and paint;
palette inheritance does not require geometry changes. Admission normalizes absent frame paint
before upstream layout, keeping explicit black distinct. Only the verified private-use negation
overlay plus equals pair maps to `≠`; arbitrary paths, fonts, scales, background fills and other
unadmitted effects refuse explicitly.

The public constants own limits: 8 KiB original source, 4,096 expanded nodes, 4,096 engine
primitives, 512 per native dimension and 32,768 reserved cells. Upstream logical nesting is limited
to 32; post-parse admission also checks expanded depth 64. MD-3 admits at most 256 formula boxes per
entry; PRE-1 bounds their aggregate reply and retained allocation. Native line wrapping is not implemented: the
unchanged wide structural fixture needs 91 columns and refuses at 88/60 rather than losing terms.

Each terminal frame admits at most 512 runs / 256 KiB; excess visible runs carry `⋮` and `Math limit`.
Each run is printable UTF-8, at most 4,096 bytes. OSC 66 script width is at most seven cells;
two-row runs reserve an even width of at most fourteen. Unsupported scale/width combinations are
local formula refusals before IPC, not malformed terminal commands. Palette and selection resolve
after geometry; explicit mathematical RGB and font treatment remain explicit.

The CLI probes through Crossterm's shared reader before creating its event stream, retaining
interleaved keys. Each ordinary missing cursor report has Crossterm's two-second timeout; this
does not prove a bound under persistent OS read errors. tmux/Screen deliberately use labelled
source; passthrough scaling and SSH portability are unproven. Old multicells are cleared from
their top row before row-ordered cell output; unchanged reservations skip cell overwrite and emit
no new native glyphs. Resize invalidates the scene. Synchronized output and saved/restored cursor
state cover the full frame; failure exits through owned terminal restoration.

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
Rust preparation path.

On 2026-09-06, the real CLI received that complete reply from one isolated loopback fixture,
prepared it in its owned child and displayed native math at 120/88/60 columns. Real SGR clicks
copied the first boxed formula with exact original delimiters through OSC 52; resize, command-palette
open/close and terminal restoration passed. The fixture disables host clipboard access and writes
no saved session. The user separately ran the native-enabled review binary and approved its live
appearance. This is not a guarantee of arbitrary TeX or KaTeX font-level fidelity.

Inspected actual workspace projections at [120](../../crates/plexmaton-tui/frames/math/reply-120.svg),
[88](../../crates/plexmaton-tui/frames/math/reply-88.svg) and
[60](../../crates/plexmaton-tui/frames/math/reply-60.svg), with matching `selection-*`, `source-*`,
`table-*` and `clipped-*` frames in that directory. These fifteen SVGs capture cells plus the same
native frame scene, not a raster transport. Root/delimiter joins remain coarse. Non-reflowing
source reveal with a keyboard path, broader terminal/font support, true partial-multicell pixels
and saturated physical-terminal latency remain unproven; whole-entry keyboard source copy exists.

Reproduce from the worktree:

```console
cargo run --offline --locked -p plexmaton-math --example native_preview -- target/math-review
python3 .agents/spikes/kitty-text-sizing/launch_macos.py --page reply --reply-directory target/math-review
cargo run --release --offline --locked -p plexmaton-math --example corpus -- --measure
cargo run --offline --locked -p plexmaton-tui --example math_preview -- target/math-workspace-review
python3 .agents/spikes/kitty-text-sizing/check_live.py --binary target/debug/plexmaton --output target/live-math-review.json
```

Release arm64 macOS, 2026-09-05: first cold batch prepares all 61 formulas in 3.914 ms and projects
183 layouts in 0.726 ms. Across 101 warm batches, preparation p50/p95/max is 1.595/2.079/2.385 ms;
projection is 0.350/0.512/0.582 ms, producing 3,063 native runs. These are standalone CPU-path
measurements, excluding terminal I/O, Markdown, worker coordination and input latency; adversarial
expansion, cancellation and aggregate memory remain separate gates.
