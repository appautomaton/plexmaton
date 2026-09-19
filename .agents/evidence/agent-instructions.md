# Evidence — Agent instructions

What proves [agent-instructions](../specs/agent-instructions.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| AGI-1 | `agi_1_global_then_project_ancestry_preserves_sources_and_exact_text`, `agi_1_no_git_and_overlapping_home_have_one_source`, `agi_1_worktree_and_symlinked_cwd_use_the_physical_checkout` |
| AGI-2 | `agi_2_missing_and_empty_files_need_no_runtime_state`, `agi_2_invalid_text_non_files_and_symlinks_refuse_loading`, `agi_2_exact_publication_bound_and_escaping_are_accounted_for`, `agi_2_cancellation_and_directory_bounds_refuse_without_a_snapshot`, `agi_2_provider_snapshot_bound_is_validated_and_redacted`; WFS-1's `bounded_reads_are_pinned_no_follow_and_cancelled_before_return`, `bounded_read_reports_completion_without_retaining_the_probe_byte` |
| AGI-3 | `agi_3_workspace_instructions_use_user_roles_in_every_dialect`, `agi_1_global_then_project_ancestry_preserves_sources_and_exact_text`, `agi_2_missing_and_empty_files_need_no_runtime_state`, `agi_5_wire_snapshot_is_stable_then_refreshes_on_jsonl_resume`; model adherence to the [scope preamble](../../crates/plexmaton-cli/src/agent_instructions/prompt.md) remains unverified |
| AGI-4 | `agi_4_instruction_bytes_are_budgeted_and_changed_rules_invalidate_measurements`, `agi_4_workspace_instructions_remain_outside_the_compaction_cut`, `cpl_2_compaction_appends_only_the_instruction_across_all_dialects`, `agi_2_provider_snapshot_bound_is_validated_and_redacted` |
| AGI-5 | `agi_5_wire_snapshot_is_stable_then_refreshes_on_jsonl_resume`, `agi_5_failed_or_cancelled_instruction_load_preserves_the_open_runtime`, `agi_5_invalid_instructions_fail_before_terminal_and_session_creation`, `agi_5_workspace_snapshot_replacement_does_not_accumulate_old_rules`, `agi_3_workspace_instructions_use_user_roles_in_every_dialect` |
