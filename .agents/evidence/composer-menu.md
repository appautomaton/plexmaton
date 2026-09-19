# Evidence — Composer menu

What proves [composer-menu](../specs/composer-menu.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| SKP-1 | `catalog_retention_is_bounded_by_count_and_bytes`, `dollar_text_is_literal_unless_a_current_skill_selection_binds_it`; `scripts/smoke-tui.py` reaches a real on-disk skill through the executable, and the crate-graph gate forbids the TUI reaching a runtime or network client at all |
| SKP-2 | `keyboard_completion_binds_numeric_names_and_token_edits_invalidate_binding`, `completion_from_inside_the_initial_token_preserves_the_request_suffix`, `retry_edit_submission_and_saved_draft_keep_independent_skill_bindings`, `unchanged_numeric_skill_retry_uses_the_historical_semantic_binding`, `numeric_retry_candidate_retains_only_typed_skill_selection` |
| SKP-3 | `escape_preserves_the_query_and_tab_inserts_the_selected_choice`, `mouse_and_wheel_choose_by_name_while_focus_stays_in_the_composer`, `variables_currency_prose_and_command_substitution_do_not_open_the_picker`, `the_listing_follows_the_leading_token` |
| SKP-4 | `short_picker_window_keeps_the_selected_tail_choice_and_controls_visible`, `the_skill_picker_frames_match_their_fixtures`; rendered review above |
| CMC-1 | `the_slash_lists_the_commands_and_only_a_whole_command_runs`, `a_requested_compaction_shows_on_the_activity_line_and_ends_with_a_note`; the runtime's CPL-9 proofs. A loopback run of `/compact` through the executable is unproven |
| CMC-2 | `the_slash_lists_the_commands_and_only_a_whole_command_runs`, `tab_completes_a_command_and_escape_keeps_the_draft`, `paste_and_unicode_inside_the_token_follow_the_same_rule`, `a_whole_draft_is_a_command_only_when_nothing_else_is_in_it`, `a_declared_flag_keeps_the_draft_a_command` |
| CMC-3 | `per_7_permission_controls_review_cancel_submit_and_refresh_by_identity`, `per_7_permission_controls_frames_keep_scope_and_confirmation_visible` with the `permission-controls-*` frames, `session_rows_live_in_the_menu_and_project_rows_in_the_drawer`, `per_7_session_setting_before_first_turn_survives_new_and_revokes_without_jsonl` |

## Rendered review

`/effort` uses [EFF-1–EFF-5](../specs/reasoning-effort.md), including its horizontal keys, disabled
stops, confirmed model state and bounded presentation clock.

Skills: the real workspace buffer was inspected at
[wide](../spikes/agent-skills/frames/skill-picker-wide.svg),
[medium](../spikes/agent-skills/frames/skill-picker-medium.svg) and
[narrow](../spikes/agent-skills/frames/skill-picker-narrow.svg) widths. Reproduce with
`cargo run -p plexmaton-tui --example skill_picker_preview -- target/skill-picker-preview`.
Commands and `/resume`: the `composer-menu-commands-medium`, `composer-menu-resume-loading-medium`
and `composer-menu-resume-medium` frames, cut from the real buffer above the composer.
`/permissions`, rows and confirmation, at three widths: the `permission-controls-wide`,
`permission-controls-medium` and `permission-controls-narrow` frames.
