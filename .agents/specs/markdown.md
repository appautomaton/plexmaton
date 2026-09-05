# Spec — Markdown transcript presentation

| Field | Value |
| --- | --- |
| Status | Implemented; three-width frames inspected, manual terminal use remains a user check |
| Owns | Assistant CommonMark projection, styled wrapping and bounded table layout |
| Depends on | TR-1–TR-4, FR-2/FR-3, SEL-2/SEL-7 |
| Proven by | TUI `markdown` and `workspace::markdown_tests` |

## Invariants

**MD-1 — Formatting does not mutate source.** Only assistant message presentation interprets
Markdown. Original-source Copy, journal and provider requests retain exact source; pointer ranges
use SEL-2's visible-text projection. User input, reasoning, system,
warning, error and tool text remain literal. No link/image/HTML execution, file access or fetching.

**MD-2 — Every styled row fits the measured width.** Headings, emphasis, lists, quotes, code and
tables share a grapheme-aware wrapper. Code keeps indentation and literal syntax; tabs display as
four spaces. A grapheme wider than the entire viewport displays a replacement marker. Tables wrap cells
or use labelled values when columns cannot fit; no value disappears to make a table fit.

**MD-3 — Incomplete streams stay readable and bounded.** CommonMark parses the current source
prefix, including unclosed fences. Formatting caps source at 128 KiB, 32,768 events, depth 32,
8,192 rows and 512 KiB of rendered text; formatting widths above 512 cells use literal source.
Tables cap 16 columns and 256 rows. Exceeding a bound
shows a named literal-source fallback; nothing is discarded from copy or context.

**MD-4 — Retained layouts are reused for interaction.** A styled-layout LRU admits at most 128
entries and 4 MiB of accounted allocation capacity, including text maps, keyed by agent, entry,
revision, width, disclosure and palette. Its widths follow TR-1's two-width height cache. Measurement and painting share rows;
hover and selection do not invalidate them. A delta replaces its old revision. An evicted layout
is rebuilt only when reached again; oversized literal fallbacks stay uncached. Height metadata
outlives layout eviction, preserving scroll geometry. `text_layouts()` counts layout/map misses.
Plain prose without any supported syntax trigger keeps the literal path; admission never parses
Markdown or infers formatting from regular expressions.

Rejected: regular-expression Markdown parsing; storing decorated text in JSONL; executing HTML or
fetching image/link targets; hiding table cells on narrow terminals; styling user instructions as
Markdown without an explicit product decision.

## Evidence

| Invariant | Proven by |
| --- | --- |
| MD-1 | `markdown_styles_blocks_and_keeps_code_literal`, `markdown_controls_and_limits_are_explicit`, `markdown_hover_copy_and_streaming_share_cached_geometry_and_exact_source`, `markdown_frames_show_messages_at_three_widths` |
| MD-2 | `markdown_tables_keep_all_values_at_wide_and_narrow_widths`, `markdown_streaming_prefixes_and_unicode_never_overflow`, `markdown_frames_show_messages_at_three_widths`, `markdown_resize_round_trip_preserves_the_parked_frame` |
| MD-3 | `markdown_controls_and_limits_are_explicit`, `markdown_streaming_prefixes_and_unicode_never_overflow` |
| MD-4 | `markdown_cache_bounds_entries_bytes_and_replaces_streamed_revisions`, `markdown_hover_copy_and_streaming_share_cached_geometry_and_exact_source`, `markdown_admission_keeps_plain_history_on_the_lightweight_path`, `markdown_resize_round_trip_preserves_the_parked_frame` |
