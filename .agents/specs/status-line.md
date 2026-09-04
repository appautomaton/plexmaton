# Spec — Status-line presentation

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | The external status command's presentation boundary |
| Depends on | BUD-1, TIM-3, JRN-7; user-approved UI/UX §input |
| Proven by | TUI `statusline::tests`, CLI `statusline::tests` and `snapshot::tests`, runtime acknowledgement test below |

## Invariants

**STL-1 — Script output becomes data, never terminal instructions.** A complete UTF-8 stdout
result is accepted only as text, line endings and the SGR grammar below, with bounded bytes, rows
and style parameters; any other control or invalid style rejects the whole result with a typed,
content-free error. Parsing produces owned Ratatui spans without execution, rendering or state changes.

Rejected: forwarding raw stdout to the terminal, which lets presentation move the cursor or write
the clipboard; and a general terminal emulator for a boundary that accepts only styled text.

**STL-2 — One owned command, with bounded replacement and shutdown.** The CLI starts only the
user-configured command, with independent timeout, cancellation, output limits and process-group
cleanup; replacement waits for the prior child and group to end, and cleanup failure stops further
execution and remains visible. Scheduling coalesces semantic changes and resizes for 300 ms without
cancelling an in-flight run: its stale result is discarded, the last rendered footer stays, and one
capture uses the latest input. It ignores streaming text deltas and never launches from rendering;
an unchanged snapshot needs no run unless the user
explicitly configured periodic refresh.

**STL-3 — Script input is an allowlisted projection of acknowledged facts.** JSON schema version 1
separates BUD-1 occupancy from TIM-3 incurred accounting and selected-path request/turn measurements;
missing values remain null or typed unavailable, with no prompt, tool content, replay payload or
credentials serialized. Resuming the same acknowledged journal produces the same durable fields;
the supplied example omits unavailable segments rather than inventing zeros.

**STL-4 — The footer never owns input or a system question.** Explicit output lines determine
height within the configured and layout caps, without wrapping; clipping reserves a visible
ellipsis cell, and equal output requests no frame. Global quit/Ctrl-P hints replace the last
terminal row without changing footer height or focus; a script failure shows a bounded diagnostic.

## Styled-output grammar

- At most 16 KiB and 64 logical rows before the separate display-height cap is applied.
- LF and CRLF terminate rows. One terminal line ending adds no extra row; interior empty rows and
  spaces are retained. Empty output has zero rows. Bare CR, tabs, DEL and C0/C1 controls are rejected.
- Only `ESC [ parameters m` is accepted: after `ESC [`, at most 128 bytes through `m` and 32 semicolon-separated
  unsigned integer parameters. Empty parameters mean zero. Colon syntax is not supported.
- SGR reset (0); bold/dim/italic/underline/reverse/strike (1/2/3/4/7/9); their resets
  (22/23/24/27/29); standard and bright foreground/background colors (30–37/40–47/90–97/100–107);
  default foreground/background (39/49); indexed and RGB colors (`38;5;n`, `48;5;n`,
  `38;2;r;g;b`, `48;2;r;g;b`), with all color channels in 0–255.
- Style carries across lines within one result, never across results. Reset restores the
  renderer's base style. No blinking, concealment, links, OSC, DCS, cursor movement or erase.

## Configuration and execution

Add `[status_line]` to the same user `config.toml` as the model registry. `command` is run by
`/bin/sh -c` in the launch working directory; using an absolute script path avoids dependence on
which project is open. `max_rows` defaults to 6 (1–64), `timeout_ms` to 1000 (10–5000).
Optional `refresh_ms` is 1000–3600000; omitted means semantic/resize refresh only. Editing a script
requires no Rust rebuild; editing its configuration requires restart.

Stdin is at most 64 KiB JSON and closes after writing. Stdout is at most 16 KiB, stderr 4 KiB;
stderr never enters the screen or a diagnostic. The configured provider-key environment variable
is removed. Other inherited environment, HOME and filesystem access remain available: user-owned
commands are not sandboxed. Cleanup signals the entire owned process group and waits up to one
second for the child and group to end. A permission-denied signal/probe during teardown does not
prove disappearance: cleanup waits within that same deadline and fails if the group persists.
A command that deliberately escapes its process group is
outside this ownership guarantee. No project configuration or script is discovered automatically.

## Snapshot fields

All fields below belong to `schema_version: 1`. Null remains distinct from zero. Claude-shaped
keys describe matching facts; this is not a promise that every Claude extension field exists.

| JSON path | Meaning |
| --- | --- |
| `cwd`, `workspace.current_dir` | Launch working directory |
| `model.id`, `.display_name`, `.provider`; `effort.level`; `thinking.enabled` | Resolved model identity and reasoning selection, without endpoint URL or credential names |
| `session_id`; `plexmaton.head`, `.created_at_unix_ms` | Acknowledged session metadata; null while acknowledgement is unavailable |
| `context_window.context_window_size`, `.used_percentage` | Configured capacity and ledger occupancy divided by capacity; not cumulative traffic |
| `context_window.total_input_tokens`, `.total_output_tokens` | Reported session traffic across all heads; coverage is `plexmaton.usage.coverage` |
| `context_window.current_usage` | Latest selected-path agent request. `input_tokens` is uncached input, derived only when both cache subsets are known; `cache_read_input_tokens`, `cache_creation_input_tokens`, `output_tokens` retain reported values |
| `cost.total_cost_usd` | Presentation float derived from known immutable ticks; null for incomplete accounting |
| `plexmaton.cost`, `.usage` | Typed canonical fixed-point cost and usage coverage/counts |
| `plexmaton.context` | Available input/reserve, measured prefix, estimated remainder, opaque heuristic bytes and estimator; otherwise an unavailable reason |
| `plexmaton.latest_request` | Attempt ID, typed owner and terminal timing/outcome/usage/cost, or null; no request content |
| `plexmaton.turn` | Latest selected turn ID, incurred usage/cost and sum of request durations; duration is null if any attempt is unresolved, and excludes idle, tools and approval wait |
| `plexmaton.terminal.columns`, `.rows` | Current terminal size; the script decides its explicit lines |

The pastel example requires Bash, jq and a Nerd Font. Its `` segment shows the latest selected-path
request's API-reported input count and percentage of configured capacity. Without a reported count,
including a fresh session, the segment is absent. It never displays a context estimate or a `ctx`/`~`
label; BUD-1 estimates remain separate snapshot data. It labels partial traffic as `reported`, omits
missing cost/cache values, and paints path components in successive pastel colors with Powerline
separators. Model, branch, cache, traffic, cost and path use ``, ``, ``, ``, `` and
``/`` respectively; `` precedes a configured reasoning level, input/output retain directional
arrows and cost retains its currency.
It queries local Git without optional locks.

## Evidence

| Invariant | Proof |
| --- | --- |
| STL-1 | `statusline_styles_and_resets_preserve_text`; `statusline_rejects_terminal_effects_and_malformed_styles`; `statusline_bounds_bytes_rows_and_parameters`; `statusline_preserves_explicit_rows_and_spaces`; `statusline_resets_restore_the_renderers_base_style`; `statusline_arbitrary_bytes_never_escape_as_controls` |
| STL-2 | `status_command_reads_snapshot_eof_and_returns_only_styled_text`; `status_command_failure_timeout_overflow_and_cancellation_are_bounded`; `status_shutdown_joins_descendants_after_a_dropped_poll`; `status_refresh_coalesces_without_cancelling_inflight_work`; `status_cleanup_waits_through_permission_denial_until_group_disappears`; `status_cleanup_never_accepts_persistent_permission_denial_as_disappearance`; `status_stale_cleanup_failure_blocks_replacement_and_remains_visible`; `status_configuration_stays_outside_the_model_registry`; `status_cleanup_error_does_not_hide_session_shutdown_failures`; `python3 scripts/smoke-statusline.py` |
| STL-3 | `status_snapshot_projects_accounting_without_prompt_or_config_and_reloads_identically`; `status_snapshot_keeps_unknown_cache_subsets_and_measurements_null`; `recorded_luna_cache_usage_survives_http_journal_resume_and_shell`; `status_script_omits_null_fields_and_keeps_rainbow_path`; `status_script_context_uses_reported_input_with_a_glyph_at_each_width`; `cancelled_model_end_during_attempt_terminal_append_keeps_the_active_owner` |
| STL-4 | `status_footer_preserves_focus_and_uses_the_last_row_for_hints`; `status_footer_clipping_reserves_a_cell_before_a_wide_grapheme`; visual frames at 120/95/60 columns approved by the user; `python3 scripts/smoke-statusline.py` |
