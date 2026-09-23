# Evidence — Compaction

What proves [compaction](../specs/compaction.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| CPL-1 | `cpl_2_compaction_appends_only_the_instruction_across_all_dialects`, `checkpoint_publication_rechecks_all_frozen_provenance`, `stale_checkpoint_publication_mutates_nothing` |
| CPL-2 | `cpl_2_compaction_appends_only_the_instruction_across_all_dialects`, `non_context_summary_failure_does_not_retry`, `cpl_2_measured_overflow_refuses_compaction_without_rewriting_history` |
| CPL-3 | `cpl_3_skill_invocation_survives_compaction_in_every_dialect`, `cpl_3_replacement_preview_rejects_oversized_summary_without_mutating_source`, `equal_atom_count_checkpoints_replace_huge_batches_and_prior_summaries`, `checkpoint_publication_rechecks_all_frozen_provenance`, `cpl_3_planning_refusals_are_typed_and_leave_source_unchanged`, `cpl_3_a_summary_that_grows_the_request_still_publishes`, `cpl_3_a_conversation_inside_its_retention_window_is_declined_until_overridden`, `cpl_3_configured_recent_tail_changes_only_the_checkpoint_cut`, `cpl_3_recent_tail_target_is_capped_by_available_input` |
| CPL-4 | `an_automatic_checkpoint_is_a_row_between_the_message_and_its_answer`, `a_checkpoint_is_one_finished_system_row`, `a_requested_compaction_shows_on_the_activity_line_and_leaves_the_saying_to_its_row`, `compaction_attempt_and_checkpoint_each_wait_for_ack_before_continuation`, `schema_2026_09_04_fixture_reopens_projects_and_continues`, `checkpoint_fixture_reopens_and_historical_fork_keeps_original_epoch`, `collected_attempt_record_round_trips_without_debugging_replay` |
| CPL-5 | `checkpoint_preserves_history_and_refreshes_the_active_step`, `repeated_checkpoints_and_historical_forks_keep_their_own_epochs`, `cpl_5_repeated_checkpoints_and_historical_forks_reopen_with_identical_wire_bytes`, `cpl_5_retention_config_change_preserves_checkpoint_and_continuation_bytes` |
| CPL-6 | `a_running_summarizer_reads_compacting_until_its_own_end`, `cpl_6_summary_http_preserves_environment_output_and_accounting_across_dialects`, `cpl_6_summary_http_rejects_tools_and_keeps_their_output_for_audit`, `cpl_6_summary_http_failures_keep_raw_terminal_and_partial_output`, `cpl_6_collector_keeps_cancelled_partial_output_and_bounds_block_growth`, `collected_attempt_validation_distinguishes_success_from_partial_failure` |
| CPL-7 | `skill_preparation_completes_while_compaction_is_waiting`, `soft_pre_turn_compaction_uses_a_distinct_owner_and_refreshes_after_checkpoint`, `post_tool_hard_pressure_preserves_history_and_dispatches_nothing`, `typed_context_error_recovers_the_same_step_once`, `context_error_after_output_does_not_start_compaction`, `summary_context_pressure_does_not_retry_with_changed_input`, `compaction_timeout_cancels_and_joins_before_continuation`, `interrupt_cancels_and_joins_the_owned_compaction`, `shutdown_cancels_and_joins_the_owned_compaction`, `interrupt_during_compaction_authorization_never_dispatches_the_summarizer`, `shutdown_during_compaction_authorization_never_dispatches_the_summarizer`, `interrupt_during_compaction_terminal_ack_starts_no_continuation`, `shutdown_during_checkpoint_ack_starts_no_agent_continuation`, `interrupt_during_refreshed_agent_authorization_starts_no_provider`, `cpl_7_turn_compaction_limits_are_bounded_independent_and_reset` |
| CPL-8 | `failed_attempt_keeps_the_frozen_source_usable`, `failed_compaction_diagnostic_reopens_without_exposing_partial_output`, `post_tool_hard_pressure_preserves_history_and_dispatches_nothing`, `uncertain_checkpoint_append_freezes_before_agent_continuation`, `cancelled_compaction_terminal_append_keeps_the_operation_owned` |
| CPL-9 | `cpl_9_requested_compaction_publishes_a_checkpoint_and_dispatches_no_step`, `cpl_9_a_running_step_refuses_the_request`, `cpl_9_an_owned_compaction_and_shutdown_refuse_the_request`, `cpl_9_a_waiting_approval_refuses_the_request`, `cpl_9_planning_refusals_are_typed_and_write_nothing`, `cpl_9_interrupt_cancels_a_requested_compaction_and_reports_it`, `cpl_9_shutdown_cancels_a_requested_compaction_and_dispatches_nothing`, `cpl_9_text_during_a_requested_compaction_waits_for_the_checkpoint`, `cpl_9_failed_and_timed_out_requests_report_their_kind_and_keep_the_head` |

## Rendered

An automatic checkpoint's turn, drawn by `cargo run -p plexmaton-tui --example
automatic_compaction_preview -- <directory>`: while the summarizer runs at
[120](../../crates/plexmaton-tui/frames/compaction/running-120.svg), [88](../../crates/plexmaton-tui/frames/compaction/running-88.svg) and [60](../../crates/plexmaton-tui/frames/compaction/running-60.svg) columns, and
afterwards with the checkpoint's row between the message and its answer at
[120](../../crates/plexmaton-tui/frames/compaction/row-in-place-120.svg), [88](../../crates/plexmaton-tui/frames/compaction/row-in-place-88.svg) and [60](../../crates/plexmaton-tui/frames/compaction/row-in-place-60.svg).
The user chose the row from these frames on 2026-09-23.
