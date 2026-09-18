# Evidence — Markdown transcript presentation

What proves [markdown](../specs/markdown.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| MD-6 | `syntax_grammars_color_language_constructs_and_preserve_every_byte`, `syntax_grammars_compile_once_and_are_shared_by_every_renderer`, `syntax_unknown_and_budget_fallbacks_keep_complete_literal_code`, `syntax_paint_selection_and_monochrome_share_exact_code_geometry`, `syntax_streaming_open_fences_remain_literal_and_finish_canonically`, `syntax_streaming_reuses_closed_fences_without_rehighlighting_the_prefix`, `syntax_event_limits_and_invalid_ranges_refuse_partial_highlights`, `syntax_workspace_selection_preserves_colors_copy_and_cached_geometry`, `text_drag_copies_wrapped_code_without_its_frame`, `markdown_theme_change_reuses_prepared_rows_and_resolves_current_colors`, `real_preparation_driver_round_trips_semantic_rows_at_three_widths`, `real_preparation_worker_reuses_streamed_markdown_prefixes`; [native frames](#native-syntax-validation) |
| MD-1 | `math_recognition_retains_original_delimiters_and_excludes_literal_regions`, `complete_reply_composes_native_math_and_exact_atomic_maps_at_three_widths`, `projection_fixture_composes_roots_and_multiline_loss_at_three_widths`, `markdown_styles_blocks_and_keeps_code_literal`, `markdown_controls_and_limits_are_explicit`, `markdown_hover_copy_and_streaming_share_cached_geometry_and_exact_source`, `markdown_frames_show_messages_at_three_widths` |
| MD-2 | `native_table_cells_keep_atomic_geometry_and_exact_tabular_copy_when_narrow`, `markdown_tables_keep_all_values_at_wide_and_narrow_widths`, `markdown_streaming_prefixes_and_unicode_never_overflow`, `frozen_prefix_accepts_an_exact_fitting_cjk_fragment`, `markdown_frames_show_messages_at_three_widths`, `markdown_resize_round_trip_preserves_the_parked_frame` |
| MD-3 | `a_math_budget_costs_geometry_and_never_the_document`, `a_formula_without_geometry_keeps_its_exact_source_and_names_why`, `streaming_math_keeps_pending_geometry_until_close_and_finalization_reveals_source`, `logits_token_stream_never_shrinks_and_finalizes_to_the_same_layout`, `native_transport_limits_refuse_locally_before_a_prepared_reply_is_encoded`, `formula_failures_are_local_typed_and_keep_source_copy_independent_of_capability`, `markdown_controls_and_limits_are_explicit`, `markdown_streaming_prefixes_and_unicode_never_overflow` |
| MD-4 | `a_finished_layout_keeps_no_growth_headroom`, `tool_transitions_do_not_reuse_stale_prepared_status`, `preparation_result_count_includes_results_dropped_by_retention`, `streaming_preparation_keeps_the_last_painted_rows_and_geometry`, `pending_preparation_reuses_only_the_latest_compatible_revision`, `pending_preparation_preserves_geometry_boundaries_and_current_refusals`, `cold_preparation_is_deferred_and_hidden_rich_history_is_not_queued`, `retained_style_accounting_includes_composed_patch_capacity`, `open_tool_repaint_reuses_preparation_and_keeps_hover_local`, `markdown_cache_bounds_entries_bytes_and_replaces_streamed_revisions`, `markdown_cache_byte_pressure_evicts_and_rebuilds_the_lru`, `markdown_cache_supplies_a_bounded_frozen_prefix_to_preparation`, `markdown_cache_advances_the_frozen_frontier_after_a_completed_tail_block`, `frozen_prefix_preserves_an_atomic_display_formula_and_copy_range`, `frozen_prefix_suffix_matrix_matches_canonical_at_three_widths`, `frozen_prefix_invalidates_when_a_late_definition_changes_the_frozen_events`, `frozen_prefix_rejects_malformed_hint_copy_ranges`, `frozen_prefix_accepts_an_exact_fitting_cjk_fragment`, `real_preparation_worker_reuses_streamed_markdown_prefixes`, `markdown_hover_copy_and_streaming_share_cached_geometry_and_exact_source`, `markdown_admission_keeps_plain_history_on_the_lightweight_path`, `markdown_resize_round_trip_preserves_the_parked_frame` |
| MD-5 | `selected_diff_keeps_semantic_colors_and_reuses_prepared_rows_at_three_widths`, `semantic_paint_keeps_custom_role_patch_order_and_nested_markdown`, `open_tool_repaint_reuses_preparation_and_keeps_hover_local`, `palette_changes_reuse_heights_and_preserve_pointer_copy_at_three_widths`, `markdown_pastel_leaves_all_workspace_roles_unchanged`, `markdown_pastel_changes_only_style_and_keeps_nested_modifiers`, `markdown_theme_change_reuses_prepared_rows_and_resolves_current_colors`; user-approved [88-column sample](../../crates/plexmaton-tui/frames/markdown-style-88.svg), with 60/120-column review frames alongside it |

## Native syntax validation

The actual workspace, prepared rows and status script were rendered and inspected at
[120](../../crates/plexmaton-tui/frames/syntax-theme/120.svg),
[88](../../crates/plexmaton-tui/frames/syntax-theme/88.svg) and
[60](../../crates/plexmaton-tui/frames/syntax-theme/60.svg) columns, plus
selected [120](../../crates/plexmaton-tui/frames/syntax-theme/selected-120.svg),
[88](../../crates/plexmaton-tui/frames/syntax-theme/selected-88.svg),
[60](../../crates/plexmaton-tui/frames/syntax-theme/selected-60.svg),
[monochrome](../../crates/plexmaton-tui/frames/syntax-theme/mono-88.svg) and
[short viewport](../../crates/plexmaton-tui/frames/syntax-theme/short-88.svg) states.
The fixture includes emphasis, inline code, a quote, Rust/Python/JSON and Chinese text.
The user has not yet reviewed this theme in their terminal; no live model requests or saved
session writes were used. Reproduce from the task checkout with:

```console
cargo run -p plexmaton-tui --example markdown_style_preview -- target/syntax-review syntax
```

## Dependency admission

Audited 2026-09-13: official Tree-sitter grammars and highlight queries for Rust 0.24.2,
Python 0.25.0, JSON 0.24.8, JavaScript 0.25.0 and TypeScript/TSX 0.23.2; Bash reuses 0.25.1.
The highlight engine stays at the workspace's 0.25.10 generation. All are MIT, bundled C with
no system-library requirement; the pinned Rust toolchain exceeds the engine's declared MSRV.
No language server, filesystem grammar discovery, injected-language loading or network access.
[Upstream highlight manifest](https://github.com/tree-sitter/tree-sitter/blob/v0.25.10/highlight/Cargo.toml)
and grammar crate manifests own dependency metadata. Resolution added six packages without replacing existing locked dependencies; affected compilation
and `cargo deny check` passed. `cargo tree -d` and feature output confirm one existing engine
generation; `cargo machete` reports no unused dependencies. Syntect's bundled-syntax path was not selected: it adds bincode, which carries
[RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html).
