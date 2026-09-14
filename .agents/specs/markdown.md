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
or use labelled values when columns cannot fit; no value disappears to make a table fit. Retained
text fragments validate their checked start-plus-grapheme width; native atoms retain MTH-1's
independent rectangle validation.

**MD-3 — Incomplete streams stay readable and bounded.** CommonMark parses the current source
prefix, including unclosed fences. Recognized unfinished math in native mode occupies one `Math…`
row until its closing delimiter arrives; finalization reveals incomplete source. MTH-1 keeps the
pending atom's exact source, including trailing newlines. Formatting caps source at 128 KiB, 32,768 events, depth 32,
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
Streaming Markdown also retains a bounded checkpoint only after a complete top-level paragraph, heading or physically closed fenced code block. The checkpoint carries source bytes, visible-text/row coordinates and a full-parser event
signature; late reference resolution, source replacement, malformed copy maps and open structural
state invalidate it.
The owned worker receives the complete source and full-parser suffix events, while layout, native
math and syntax work before the checkpoint is reused. A completed entry takes the canonical full path.
Checkpoint source/layout hints are capped at 64 KiB and are omitted when that bound cannot be met;
the existing request/reply bounds remain authoritative.
Hidden plain prose without any supported syntax trigger keeps count-only measurement; admission never parses
Markdown or infers formatting from regular expressions.

**MD-5 — Color resolves from semantic style intent without reflow.** Prepared text retains ordered
workspace/Markdown/code role and modifier patches, never resolved terminal colors; painting uses the
current palette without parsing, wrapping or rebuilding copy fragments. The explicit Markdown
theme belongs to palette identity, independently of workspace chrome; its inherited choice follows
the workspace palette, including monochrome. The designed palette's Markdown is the same named
tokens: sky, mint and teal headings, sky links and gold inline code. Without truecolor the chrome
keeps its ANSI slots and Markdown keeps the tokens.

Rejected: regular-expression Markdown parsing; storing decorated text in JSONL; executing HTML or
fetching image/link targets; hiding table cells on narrow terminals; styling user instructions as
Markdown without an explicit product decision.

**MD-6 — Syntax is bounded presentation of literal code.** Recognized fenced languages prepare
semantic code roles inside PRE-1/PRE-2's owned worker, never in draw or input; unknown or unlabelled
code remains literal. A 32 KiB per-block code budget and bounded highlight events degrade a whole
block to visibly labelled plain code, preserving all text; incomplete syntax remains readable and
selection retains token distinctions. The code frame encloses the current presentation; it does
not claim a physical closing fence has arrived. Only a physically closed top-level fence can be
frozen under MD-4.

## Code theme

The fence's first info word selects Rust (`rs`), Python (`py`, `python3`), JSON (`jsonc`),
JavaScript (`js`, `jsx`), TypeScript (`ts`, `tsx`) or Bash (`sh`, `shell`), case-insensitively.
Empty, `text`, `txt`, `plaintext` and unknown info words keep plain code. This is grammar-based
syntax classification, not language-server semantic analysis. No automatic language guessing.
A whole code block above 32 KiB, more than 32,768 highlight events or more than 128 nested
captures loses only highlighting, with an explicit plain-text label. MD-3 bounds the complete
entry; PRE-2 bounds computation, replacement and shutdown. The byte and event budgets bound the
work, not its wall-clock time, which is superlinear in block size: adversarial punctuation at the
32 KiB cap costs seconds, while real code at that cap costs milliseconds. Open blocks reparse only
when their revision reaches the coalescing worker; unchanged paint never parses. This is not an
incremental syntax-tree cache. Each bundled grammar's query is compiled at most once per process
and then only read, because compiling one costs two orders of magnitude more than highlighting an
ordinary block with it.

| Code role | Pastel token |
| --- | --- |
| Text, variables, operators and punctuation | Body |
| Keywords | Sky |
| Types and properties, including JSON keys | Teal |
| Functions and macros | Gold |
| Strings | Mint |
| Numbers and constants | Orange |
| Comments | Steel, italic |

These are content roles, not workspace attention states. Inherited palettes use Body with bold
keywords and muted italic comments, retaining a color-free monochrome path. Headings, bold,
italic, quotes and links keep MD-5's styles; inline code adds the existing Bar background.
Pointer and entry selection use the existing Bar background for pastel Markdown, preserving
foreground colors and emphasis; inherited and monochrome themes retain the workspace Selection
style. Selection padding retains its measured width. Source Copy and pointer
Copy continue to use MD-1/SEL-2. Rejected: terminal-colored spans in preparation, which would
require parsing again when the palette changes.

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

## Evidence

| Invariant | Proven by |
| --- | --- |
| MD-6 | `syntax_grammars_color_language_constructs_and_preserve_every_byte`, `syntax_grammars_compile_once_and_are_shared_by_every_renderer`, `syntax_unknown_and_budget_fallbacks_keep_complete_literal_code`, `syntax_paint_selection_and_monochrome_share_exact_code_geometry`, `syntax_streaming_open_fences_remain_literal_and_finish_canonically`, `syntax_streaming_reuses_closed_fences_without_rehighlighting_the_prefix`, `syntax_event_limits_and_invalid_ranges_refuse_partial_highlights`, `syntax_workspace_selection_preserves_colors_copy_and_cached_geometry`, `text_drag_copies_wrapped_code_without_its_frame`, `markdown_theme_change_reuses_prepared_rows_and_resolves_current_colors`, `real_preparation_driver_round_trips_semantic_rows_at_three_widths`, `real_preparation_worker_reuses_streamed_markdown_prefixes`; [native frames](#native-syntax-validation) |
| MD-1 | `math_recognition_retains_original_delimiters_and_excludes_literal_regions`, `complete_reply_composes_native_math_and_exact_atomic_maps_at_three_widths`, `projection_fixture_composes_roots_and_multiline_loss_at_three_widths`, `markdown_styles_blocks_and_keeps_code_literal`, `markdown_controls_and_limits_are_explicit`, `markdown_hover_copy_and_streaming_share_cached_geometry_and_exact_source`, `markdown_frames_show_messages_at_three_widths` |
| MD-2 | `native_table_cells_keep_atomic_geometry_and_exact_tabular_copy_when_narrow`, `markdown_tables_keep_all_values_at_wide_and_narrow_widths`, `markdown_streaming_prefixes_and_unicode_never_overflow`, `frozen_prefix_accepts_an_exact_fitting_cjk_fragment`, `markdown_frames_show_messages_at_three_widths`, `markdown_resize_round_trip_preserves_the_parked_frame` |
| MD-3 | `streaming_math_keeps_pending_geometry_until_close_and_finalization_reveals_source`, `logits_token_stream_never_shrinks_and_finalizes_to_the_same_layout`, `native_transport_limits_refuse_locally_before_a_prepared_reply_is_encoded`, `formula_failures_are_local_typed_and_keep_source_copy_independent_of_capability`, `markdown_controls_and_limits_are_explicit`, `markdown_streaming_prefixes_and_unicode_never_overflow` |
| MD-4 | `tool_transitions_do_not_reuse_stale_prepared_status`, `preparation_result_count_includes_results_dropped_by_retention`, `streaming_preparation_keeps_the_last_painted_rows_and_geometry`, `pending_preparation_reuses_only_the_latest_compatible_revision`, `pending_preparation_preserves_geometry_boundaries_and_current_refusals`, `cold_preparation_is_deferred_and_hidden_rich_history_is_not_queued`, `retained_style_accounting_includes_composed_patch_capacity`, `open_tool_repaint_reuses_preparation_and_keeps_hover_local`, `markdown_cache_bounds_entries_bytes_and_replaces_streamed_revisions`, `markdown_cache_byte_pressure_evicts_and_rebuilds_the_lru`, `markdown_cache_supplies_a_bounded_frozen_prefix_to_preparation`, `markdown_cache_advances_the_frozen_frontier_after_a_completed_tail_block`, `frozen_prefix_preserves_an_atomic_display_formula_and_copy_range`, `frozen_prefix_suffix_matrix_matches_canonical_at_three_widths`, `frozen_prefix_invalidates_when_a_late_definition_changes_the_frozen_events`, `frozen_prefix_rejects_malformed_hint_copy_ranges`, `frozen_prefix_accepts_an_exact_fitting_cjk_fragment`, `real_preparation_worker_reuses_streamed_markdown_prefixes`, `markdown_hover_copy_and_streaming_share_cached_geometry_and_exact_source`, `markdown_admission_keeps_plain_history_on_the_lightweight_path`, `markdown_resize_round_trip_preserves_the_parked_frame` |
| MD-5 | `selected_diff_keeps_semantic_colors_and_reuses_prepared_rows_at_three_widths`, `semantic_paint_keeps_custom_role_patch_order_and_nested_markdown`, `open_tool_repaint_reuses_preparation_and_keeps_hover_local`, `palette_changes_reuse_heights_and_preserve_pointer_copy_at_three_widths`, `markdown_pastel_leaves_all_workspace_roles_unchanged`, `markdown_pastel_changes_only_style_and_keeps_nested_modifiers`, `markdown_theme_change_reuses_prepared_rows_and_resolves_current_colors`; user-approved [88-column sample](../../crates/plexmaton-tui/frames/markdown-style-88.svg), with 60/120-column review frames alongside it |
