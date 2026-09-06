# Spec — Markdown transcript presentation

| Field | Value |
| --- | --- |
| Status | Implemented; three-width frames inspected, manual terminal use remains a user check |
| Owns | Assistant CommonMark projection, bounded table layout and the shared prepared-text cache |
| Depends on | TR-1–TR-4, FR-2/FR-3, SEL-2/SEL-7 |
| Proven by | TUI `markdown` and `workspace::markdown_tests` |

## Invariants

**MD-1 — Formatting does not mutate source.** Only assistant message presentation interprets
Markdown. Original-source Copy, journal and provider requests retain exact source; pointer ranges
use SEL-2's visible-text projection. User input, reasoning, system,
warning, error and tool text remain literal. No link/image/HTML execution, file access or fetching.
Assistant math recognizes paired dollar spans through CommonMark and source-mapped `\(…\)` /
`\[…\]` outside code, HTML and link/image literals. MTH-1 owns their atomic original source.

**MD-2 — Every styled row fits the measured width.** Headings, emphasis, lists, quotes, code and
tables share a grapheme-aware wrapper. Code keeps indentation and literal syntax; tabs display as
four spaces. A grapheme wider than the entire viewport displays a replacement marker. Tables wrap cells
or use labelled values when columns cannot fit; no value disappears to make a table fit.

**MD-3 — Incomplete streams stay readable and bounded.** CommonMark parses the current source
prefix, including unclosed fences. Formatting caps source at 128 KiB, 32,768 events, depth 32,
8,192 rows and 512 KiB of rendered text; formatting widths above 512 cells use literal source.
Tables cap 16 columns and 256 rows; entries cap 256 formula boxes. Exceeding a bound
shows a named literal-source fallback; nothing is discarded from copy or context.

**MD-4 — Retained preparation is reused for interaction and paint.** A palette-independent LRU admits at most 128
layout-version slots and 4 MiB of accounted allocation capacity, including text maps and composed style layers,
keyed by agent, entry, revision, width, disclosure and math capability. Native runs and atom maps
share this accounting; its widths follow TR-1's two-width height cache.
All transcript entry kinds share asynchronously prepared rows between painting and pointer mapping;
rich messages and disclosed tools also reuse them for measurement. Hover, selection and palette
changes do not invalidate preparation. Each geometry retains at most its two newest revisions
under the same LRU bound; finalization can leave both the last streaming and final versions retained.
While append-only text is pending, measurement and paint reuse the newest successful compatible
revision no newer than the source, pinning its own key and rows; the current revision is still
requested. Tools, artifacts and mail require an exact revision because their fields are current
facts, not text prefixes (ENT-2/ENT-3). Cached outcomes retain their key on both success and refusal;
oversized lookup identities refuse before key allocation. An exact refusal remains visible. An evicted layout is
requested when reached again or needed for selected-text copy (PRE-3/PRE-4), never rebuilt by an
input handler. PRE-1's allocation limit gives oversized entries a named refusal with source copy
intact. Height metadata outlives layout eviction and palette replacement. `text_layouts()` counts
admitted preparation results delivered to the cache, including typed refusals and results dropped
by retention; it does not count occupied slots or parser invocations.
Hidden plain prose without any supported syntax trigger keeps count-only measurement; admission never parses
Markdown or infers formatting from regular expressions.

**MD-5 — Color resolves from semantic style intent without reflow.** Prepared text retains ordered
workspace/Markdown role and modifier patches, never resolved terminal colors; painting uses the
current palette without parsing, wrapping or rebuilding copy fragments. The explicit Markdown
theme belongs to palette identity, independently of workspace chrome; its inherited choice follows
the workspace palette, including monochrome. The
user-approved CLI default uses the existing pastel blue/green/lavender heading accents, teal links
and warm-yellow inline code, while body text and terminal-owned chrome stay neutral/ANSI.

Rejected: regular-expression Markdown parsing; storing decorated text in JSONL; executing HTML or
fetching image/link targets; hiding table cells on narrow terminals; styling user instructions as
Markdown without an explicit product decision.

## Evidence

| Invariant | Proven by |
| --- | --- |
| MD-1 | `math_recognition_retains_original_delimiters_and_excludes_literal_regions`, `complete_reply_composes_native_math_and_exact_atomic_maps_at_three_widths`, `markdown_styles_blocks_and_keeps_code_literal`, `markdown_controls_and_limits_are_explicit`, `markdown_hover_copy_and_streaming_share_cached_geometry_and_exact_source`, `markdown_frames_show_messages_at_three_widths` |
| MD-2 | `native_table_cells_keep_atomic_geometry_and_exact_tabular_copy_when_narrow`, `markdown_tables_keep_all_values_at_wide_and_narrow_widths`, `markdown_streaming_prefixes_and_unicode_never_overflow`, `markdown_frames_show_messages_at_three_widths`, `markdown_resize_round_trip_preserves_the_parked_frame` |
| MD-3 | `native_transport_limits_refuse_locally_before_a_prepared_reply_is_encoded`, `formula_failures_are_local_typed_and_keep_source_copy_independent_of_capability`, `markdown_controls_and_limits_are_explicit`, `markdown_streaming_prefixes_and_unicode_never_overflow` |
| MD-4 | `tool_transitions_do_not_reuse_stale_prepared_status`, `preparation_result_count_includes_results_dropped_by_retention`, `streaming_preparation_keeps_the_last_painted_rows_and_geometry`, `pending_preparation_reuses_only_the_latest_compatible_revision`, `pending_preparation_preserves_geometry_boundaries_and_current_refusals`, `cold_preparation_is_deferred_and_hidden_rich_history_is_not_queued`, `retained_style_accounting_includes_composed_patch_capacity`, `open_tool_repaint_reuses_preparation_and_keeps_hover_local`, `markdown_cache_bounds_entries_bytes_and_replaces_streamed_revisions`, `markdown_cache_byte_pressure_evicts_and_rebuilds_the_lru`, `markdown_hover_copy_and_streaming_share_cached_geometry_and_exact_source`, `markdown_admission_keeps_plain_history_on_the_lightweight_path`, `markdown_resize_round_trip_preserves_the_parked_frame` |
| MD-5 | `selected_diff_keeps_semantic_colors_and_reuses_prepared_rows_at_three_widths`, `semantic_paint_keeps_custom_role_patch_order_and_nested_markdown`, `open_tool_repaint_reuses_preparation_and_keeps_hover_local`, `palette_changes_reuse_heights_and_preserve_pointer_copy_at_three_widths`, `markdown_pastel_leaves_all_workspace_roles_unchanged`, `markdown_pastel_changes_only_style_and_keeps_nested_modifiers`, `markdown_theme_change_reuses_prepared_rows_and_resolves_current_colors`; user-approved [88-column sample](../../crates/plexmaton-tui/frames/markdown-style-88.svg), with 60/120-column review frames alongside it |
