# Evidence — Foreground command tool

What proves [command-tool](../specs/command-tool.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| CMD-1 | `cmd_1_model_schema_is_strict_nullable_and_exposes_timeout_bounds`, `cmd_1_admission_is_strict_canonical_and_pins_the_workspace`, `cmd_1_refuses_every_shape_outside_the_model_contract_and_hard_bounds`, `cmd_1_multibyte_and_escaped_commands_fit_every_advertised_bound`, `cmd_1_approval_detail_leads_with_command_and_bounds_root_separately`, `native_command_is_visible_before_decision_at_the_smallest_terminal`, `cmd_1_executor_refuses_a_call_pinned_to_another_workspace`, `cmd_1_changed_workspace_identity_is_refused_before_spawn` |
| CMD-2 | `cmd_2_and_cmd_4_use_fixed_noninteractive_context_and_typed_exit`, `cmd_2_snapshot_preserves_path_and_home_but_scrubs_private_authority`, `cmd_2_environment_snapshot_preserves_non_unicode_entries`, `cmd_2_environment_snapshot_scrubs_credential_shaped_names`, `cmd_2_selected_api_key_environment_is_removed_even_without_credential_shape`, `model_credentials_are_removed_before_install_and_fingerprinting` |
| CMD-3 | `cmd_3_capture_keeps_exact_raw_head_tail_and_omission_across_chunking`, `cmd_3_utf8_projection_keeps_a_character_split_between_head_and_tail`, `cmd_3_drains_one_mibibyte_from_each_pipe_after_retention_fills`, `cmd_3_completed_drain_is_not_polled_again_when_the_sibling_is_sealed`, `cmd_3_preserves_invalid_utf8_as_raw_bytes_and_bounds_its_text_view`, `cmd_3_escaped_pipe_holder_is_sealed_and_joined_with_partial_evidence`, `cmd_3_and_cmd_4_model_formatter_is_typed_and_hard_bounded_after_lossy_utf8` |
| CMD-4 | `cmd_2_and_cmd_4_use_fixed_noninteractive_context_and_typed_exit`, `cmd_3_and_cmd_4_model_formatter_is_typed_and_hard_bounded_after_lossy_utf8`, `cmd_5_timeout_has_a_typed_cause_and_leaves_no_process_group`, `cmd_5_cancellation_gracefully_terms_reaps_and_joins_drains` |
| CMD-5 | `cmd_5_cancellation_gracefully_terms_reaps_and_joins_drains`, `cmd_5_timeout_has_a_typed_cause_and_leaves_no_process_group`, `cmd_5_interrupt_wins_when_timeout_is_simultaneously_ready`, `cmd_5_deadline_wins_when_completion_is_already_ready`, `cmd_5_group_disappearing_at_sigkill_does_not_report_it_sent`, `cmd_5_permission_denied_probe_still_reports_an_existing_group`, `cmd_5_root_exit_terminates_a_descendant_holding_an_inherited_pipe`, `cmd_5_signal_failure_retains_primary_error_and_reaps_the_owned_group`, `cmd_5_kill_failure_is_retried_while_retaining_the_primary_error`, `cmd_5_inspect_failure_retains_primary_error_and_reaps_the_owned_group` |
| CMD-6 | `cmd_6_pre_cancelled_command_never_spawns`, `cmd_5_wait_failure_retains_primary_error_and_reaps_the_owned_group`, `cmd_5_try_wait_failure_retains_primary_error_and_reaps_the_owned_group`, `cmd_3_escaped_pipe_holder_is_sealed_and_joined_with_partial_evidence`, `cancelled_next_event_keeps_command_work_owned_until_interrupt_joins_it`, `cancelled_shutdown_can_be_called_again_to_finish_exact_cleanup`, `dropping_an_active_runtime_joins_its_command_worker_and_process_group` |
| CMD-7 | `cmd_7_writes_outside_the_granted_roots_are_denied_by_the_os`, `every_root_reaching_a_profile_is_its_own_resolved_form`, `absent_roots_are_dropped_rather_than_granted`, `roots_are_deduplicated`, `profile_grants_follow_the_global_deny`, `profile_without_roots_still_grants_the_stateless_devices`, `stateless_devices_are_granted_by_path_and_never_as_a_tree`, `enforced_wraps_the_shell_and_terminates_its_own_arguments`, `unconfined_launches_the_shell_unchanged`, `a_binding_carries_the_whole_path_after_one_equals` |

The denial itself is host-dependent: `cmd_7_…_denied_by_the_os` asserts the fence where a profile
can be applied, and asserts an unchanged spawn where one cannot, so neither host leaves the claim
untested. What the kernel does with a profile is proven outside the suite by
`.agents/spikes/permission-policy/seatbelt-probe.py` (13 checks) and `seatbelt-lifecycle.py`
(24 checks, each arm bare and wrapped).
