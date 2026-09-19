# Evidence — Native math layout

What proves [math-layout](../specs/math-layout.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proof |
| --- | --- |
| MTH-1 | `streaming_math_keeps_pending_geometry_until_close_and_finalization_reveals_source`, `formula_hit_cells_are_atomic_and_preserve_original_delimiters`, `formula_clicks_and_reverse_edge_drags_select_highlight_and_copy_the_complete_source`, `an_atomic_range_highlights_every_blank_and_edge_cell`, `formula_source_fallback_and_reflow_preserve_atomic_selection_without_repreparing_for_paint`, `streamed_formula_completion_and_markdown_reinterpretation_cannot_leave_partial_tex_selected`, `reported_roots_and_log_sum_exp_loss_project_at_three_widths`, `real_preparation_worker_preserves_the_projection_reply`, `real_preparation_worker_preserves_the_complete_native_math_reply` |
| MTH-2 | `every_admitted_accent_merges_into_one_cell_and_a_path_accent_refuses`, `an_adjacent_term_never_collides_with_a_neighbours_reservation`, `the_reported_heat_transfer_formulas_lay_out_natively`, `cjk_scripts_and_single_base_accents_keep_unicode_scale_and_paint`, `unsupported_group_accents_and_explicit_cjk_fonts_refuse_before_projection`, `logits_accents_preserve_prediction_and_gradient_at_three_widths`, `logits_cjk_labels_preserve_all_text_and_box_at_three_widths`, `real_preparation_worker_preserves_the_logits_math_reply`, `complete_attention_reply_preserves_all_formula_occurrences_at_three_widths`, `structural_corpus_preserves_tables_roots_and_explicit_overflow`, `fraction_rows_and_paired_scripts_retain_engine_geometry`, `font_glyph_mapping_preserves_not_equal_double_struck_and_macron`, `explicit_colors_do_not_become_palette_inheritance`, `framed_paint_inherits_without_erasing_explicit_color`, `radicals_span_the_radicand_and_text_keeps_word_gaps`, `tall_and_indexed_roots_keep_bounded_nonoverlapping_geometry`, `unrelated_nested_scripts_stay_in_the_numerator`, `compound_root_indices_keep_their_complete_group`, `script_roots_do_not_use_full_size_radicals`, `reported_roots_and_log_sum_exp_loss_project_at_three_widths`, `non_parenthesis_vector_paths_remain_a_typed_refusal`, `independent_native_overprint_is_refused` |
| MTH-3 | `native_runs_keep_their_origin_and_never_cross_viewport_or_overlay_edges`, `native_table_cells_keep_atomic_geometry_and_exact_tabular_copy_when_narrow`, `complete_reply_composes_native_math_and_exact_atomic_maps_at_three_widths`; real partial-multicell pixel fidelity remains unproven |
| MTH-4 | `root_index_normalization_bounds_layout_nodes`, `source_and_native_limits_refuse_without_truncation_or_macro_leakage`, `aggregate_cell_bound_is_checked_before_paint_allocation`, `native_reply_roundtrip_validates_the_complete_corpus_and_rejects_forged_runs`, `native_transport_limits_refuse_locally_before_a_prepared_reply_is_encoded`, `preparation_wire_rejects_mismatched_math_capability_geometry_and_atomic_maps`, `replacing_projection_revokes_math_work_even_when_semantic_keys_are_identical`; PRE-2 owns process timeout/replacement/shutdown evidence |
| MTH-5 | `streaming_preparation_preserves_native_runs_without_rewriting_them`, `cells_native_math_and_clipboard_share_one_ordered_output_owner`, `native_encoder_rejects_invalid_scale_controls_and_capacity_before_output`, `native_capability_requires_the_complete_measured_cursor_sequence`, `failed_native_output_keeps_the_last_painted_hit_map_and_frame_identity`, `formula_source_fallback_and_reflow_preserve_atomic_selection_without_repreparing_for_paint`; direct-Kitty CLI evidence below |

## Rendered and terminal evidence

The [complete reply fixture](../../crates/plexmaton-math/fixtures/attention-derivatives.json) retains
61 verified byte ranges: 35 display and 26 inline occurrences. All prepare/project at 120/88/60.
The review example composes them with the original prose; Markdown markers remain visible because
it does not implement another Markdown renderer. Its page boundaries keep formulas whole.

Reviewed native-run projections at [120](../../crates/plexmaton-math/frames/native-120.svg),
[88](../../crates/plexmaton-math/frames/native-88.svg),
[60](../../crates/plexmaton-math/frames/native-60.svg) and
[derivatives](../../crates/plexmaton-math/frames/native-derivatives-88.svg) show the native cell
reservations. SVG is review evidence, never the
application's formula transport, and is not a pixel capture of Kitty.

The [logits fixture](../../crates/plexmaton-math/fixtures/logits.json) preserves the user's four
formulas. Reviewed actual workspace output at
[120](../../crates/plexmaton-tui/frames/math/logits-120.svg),
[88](../../crates/plexmaton-tui/frames/math/logits-88.svg) and
[60](../../crates/plexmaton-tui/frames/math/logits-60.svg), plus the pending formula at
[120](../../crates/plexmaton-tui/frames/math/logits-pending-120.svg),
[88](../../crates/plexmaton-tui/frames/math/logits-pending-88.svg) and
[60](../../crates/plexmaton-tui/frames/math/logits-pending-60.svg).
`math_preview -- target/latex-review --logits` reproduces these frames, exact boxed-formula copy
and stopped-stream source. These projections do not establish live physical-terminal flicker.
`math_preview -- target/math-workspace-review --projection` renders the source-linked square-root
and multiline-loss fixture with an exact-loss click-copy check. Inspected workspace projections at
[120](../../crates/plexmaton-tui/frames/math/projection-120.svg),
[88](../../crates/plexmaton-tui/frames/math/projection-88.svg), and
[60](../../crates/plexmaton-tui/frames/math/projection-60.svg) show short-root joins and both loss sums.
The same command exports root-index regressions at
[120](../../crates/plexmaton-tui/frames/math/indices-120.svg),
[88](../../crates/plexmaton-tui/frames/math/indices-88.svg), and
[60](../../crates/plexmaton-tui/frames/math/indices-60.svg). Compound indices retain their terms and
nested scripts stay above the fraction bar. Tall and script radicals remain coarse cell projections;
these SVGs do not establish physical-terminal pixel fidelity.

On 2026-09-05, direct Kitty 0.46.1 / Menlo 15 consumed eight full-reply pages at each width, with
all 61 formula occurrences covered, explicit redraw and clean exit. The
[owned review transport](../spikes/kitty-text-sizing/README.md) sends native Unicode/OSC 66 from
the Rust export, not manually placed equation scenes. Nineteen Python/PTY checks include the real
Rust preparation path.

On 2026-09-06, the real CLI received that complete reply from one isolated loopback fixture,
prepared it in its owned child and displayed native math at 120/88/60 columns. Real SGR clicks
copied the first boxed formula with exact original delimiters through OSC 52; resize, Drawer
open/close and terminal restoration passed. The fixture disables host clipboard access and writes
no saved session. The user separately ran the native-enabled review binary and approved its live
appearance. This is not a guarantee of arbitrary TeX or KaTeX font-level fidelity.

Inspected actual workspace projections at [120](../../crates/plexmaton-tui/frames/math/reply-120.svg),
[88](../../crates/plexmaton-tui/frames/math/reply-88.svg) and
[60](../../crates/plexmaton-tui/frames/math/reply-60.svg), with matching `selection-*`, `source-*`,
`table-*` and `clipped-*` frames in that directory. These fifteen SVGs capture cells plus the same
native frame scene, not a raster transport. Non-reflowing
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

## Dependency admission

The exact 0.1.14 parser/layout/types/font pins resolve a five-crate RaTeX core including its lexer.
Its tall-parenthesis adapter is deliberately coupled to that release's stacked-delimiter command
signature and 0.875 em box; a release change is a typed path refusal until this evidence is audited.
The root-index adapter also depends on that release's `index_offset`, `index_scale` and 5/18-em
index placement; an upgrade must audit the index reservation and compound/nested-script evidence.
Published packages are MIT, edition 2021, without a declared MSRV; they build on workspace Rust
1.98.0. The [source](https://github.com/erweixin/RaTeX/tree/c902516816cdc84519827d8b46d1cd40270d0451)
was active on 2026-09-04. No browser, image backend, native library or font-file loader enters the
graph. Native output uses the terminal's fonts, not the source repository's separate font files.
New transitive thiserror 1 coexists with first-party 2; terminal foundations are unchanged.
Offline cargo-deny passed using the cached advisory database; feature-tree inspection and
cargo-machete passed. No claim of a freshly fetched advisory database follows from the offline gate.
