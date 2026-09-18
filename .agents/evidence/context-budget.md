# Evidence — Context budget

What proves [context-budget](../specs/context-budget.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| BUD-1 | `bud_1_both_codecs_produce_redacted_deterministic_ledgers_without_writes`, `bud_1_runtime_snapshot_uses_the_configured_model_without_dispatch`, `bud_1_incomplete_tool_batch_cannot_produce_a_fit_snapshot`, `bud_2_anchor_uses_exact_input_and_survives_record_reload`, `cancelled_model_end_during_attempt_terminal_append_keeps_the_active_owner`, `failed_user_append_returns_the_draft_and_starts_no_effect`, `dropping_the_runtime_joins_its_journal_writer` |
| BUD-2 | `bud_2_anchor_uses_exact_input_and_survives_record_reload`, `bud_2_missing_and_changed_environment_have_no_anchor`, `bud_2_exact_input_with_missing_breakdowns_remains_an_anchor`, `bud_2_longest_prefix_wins_and_other_branches_are_excluded`, `bud_2_compaction_measurements_do_not_anchor_agent_context`, `bud_2_parallel_batch_anchors_require_every_result_in_model_order`, `bud_2_codec_environment_controls_measurement_reuse`, `bud_2_measured_prefix_replaces_estimates_without_double_counting_environment`, `checkpoint_preserves_history_and_refreshes_the_active_step`, `repeated_checkpoints_and_historical_forks_keep_their_own_epochs` |
| BUD-3 | `bud_3_unmeasured_and_opaque_inputs_keep_their_estimate_provenance`, `bud_3_opaque_replay_is_flagged_and_incompatibility_never_becomes_a_zero_estimate`, `bud_3_maximal_tool_results_are_estimated_as_one_indivisible_atom`, `bud_3_estimator_counts_utf8_wire_bytes_without_allocating_another_request_string`, `cpl_2_measured_overflow_refuses_compaction_without_rewriting_history` |
| BUD-4 | `bud_4_decisions_cover_soft_hard_reserve_and_indivisible_boundaries`, `bud_4_invalid_limits_anchors_and_overflow_are_typed`, `bud_2_measured_prefix_replaces_estimates_without_double_counting_environment` |
