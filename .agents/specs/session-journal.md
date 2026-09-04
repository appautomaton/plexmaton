# Spec — Session journal

| Field | Value |
| --- | --- |
| Status | Implemented through Phase 02 stage 2 slice 3 |
| Owns | Typed journal, heads, projections, wire, writer, recovery and commit/effect ordering |
| Depends on | PRV-3/PRV-4 for model replay, ENT-1/ENT-3 for transcript identity and pure reduction |
| Proven by | Agent, store, runtime and composition tests |

## Invariants

**JRN-1 — One record is one complete mutation.** An append carries its parent, head and expected
revision together, so it advances that head or changes nothing. Move, rename and abandon are also
revision-checked; create requires a fresh, never-reused name. Sequence, record and entry identities
never repeat. Terminal records check their head/boundary but advance neither.

**JRN-2 — Reduction is deterministic and typed.** Equal ordered records build equal entries, heads
and paths. Gaps, unknown ancestry, stale revisions and reused identities are typed refusals that
mutate nothing; model steps are one-based and contiguous within a turn; turn terminals additionally
reject missing, duplicate or mismatched ownership.

**JRN-3 — The wire is lossless without weakening opaque replay.** Records use tagged JSON decoded
through validating constructors. One `AssistantOutput` retains ordered text, reasoning and calls;
its block-anchored replay carries route owner, codec revision and model family. Serialization keeps
exact `ProviderReplay` bytes; `Debug` and decode errors reveal none. The header names
`plexmaton.session`, the `2026-09-04`
schema epoch, session identity and externally observed `created_at_unix_ms`; file and in-memory
journal retain the same metadata. A foreign epoch fails before record decoding. Rejected: rolling
numeric versions, migration readers and dual canonical payloads before a public compatibility
contract exists.

**JRN-4 — One record is one write, and loading keeps a valid prefix.** The adapter uses one
unbuffered `write` per record and no `fsync`. A returned append survives process death; power loss
may discard the unflushed tail. Load repairs complete final JSON without a newline, isolates an
incomplete tail, and stops at earlier corruption. Journal, staging and isolated-tail files are
owner-only. Session names resolve beneath an owner-only `PLEXMATON_HOME/sessions`.
Normal startup creates its JSONL file; only explicit `--ephemeral` declines file persistence.
Automatic identities are portable UUIDv7 names while `created_at_unix_ms` remains independently
injected chronology; explicit portable string identities remain valid. Rejected: timestamp-only
automatic identities, which collide when stores combine.
Rejected: per-record `fsync` or macOS `F_FULLFSYNC` for a stronger power-loss promise; and a
database or on-disk index before a measured query need.

**JRN-5 — Replay performs no effects.** One head derives ordered, indivisible `ContextAtom`s and a
numbered event stream without invoking providers, tools, policy or files. A complete assistant
output plus every result is one `ToolBatch`; results follow call order despite out-of-order terminal
transitions. An incomplete final batch stays visible, is wholly omitted from provider input and
yields typed recovery; before a later model fact it is corruption.
Resume settles orphaned calls with a stable `process_died` result and marks the turn interrupted
before new work. Completed messages normalize transport chunks into one replay delta.

**JRN-6 — Completed live facts enter once.** Each settled fact enters as one `JournalRecord` returned
in its `Reaction`; requests read only the selected path. Streaming deltas use a delivery cursor, but
only the completed message is canonical. Rebuilding idle state rebases that cursor; active transient
work refuses rebuilding.

**JRN-7 — Acknowledged records precede dependent effects.** The runtime stages each
transition through one bounded writer, publishing events and starting effects only after appends
return. Cancelled waits stay owned; shutdown joins accepted appends and the writer. Failure returns
text in arrival order, distinguishes unwritten from unknown outcomes, reports cleanup and requires
reopen. The TUI loop performs no filesystem operation.

## Evidence

| Invariant | Proven by |
| --- | --- |
| JRN-1 | `jrn_1_append_and_head_mutations_form_one_checked_tree` |
| JRN-2 | `jrn_2_invalid_records_change_nothing`, `jrn_2_the_same_records_build_equal_journals_and_paths`, `jrn_2_each_head_mutation_rejects_a_stale_revision`, `jrn_2_each_head_mutation_rejects_a_missing_head`, `jrn_2_an_unknown_append_parent_is_a_missing_entry`, `jrn_2_head_names_are_never_reused`, `jrn_2_model_steps_cannot_skip_rewind_or_repeat`, `tim_1_invalid_terminal_records_change_nothing`, `tim_1_partial_head_mutations_preserve_or_refuse_the_open_turn`, `tim_1_jsonl_rejects_untimed_turns_and_unscoped_lifecycle_records` |
| JRN-3 | `jrn_3_every_record_round_trips_and_debug_redacts_replay`, `jrn_3_every_context_block_variant_round_trips_inside_an_append`, `jrn_3_every_canonical_payload_variant_round_trips_inside_an_append`, `jrn_3_decoding_rechecks_identity_and_replay_bounds`, `assistant_output_round_trips_order_and_redacts_replay`, `assistant_output_rejects_duplicate_semantic_identities`, `assistant_output_rechecks_aggregate_text_and_tool_identity_bounds`, `oversized_semantic_text_fails_before_canonical_commit`, `maximal_valid_assistant_output_fits_the_journal_line_envelope`, `jrn_3_and_jrn_4_encrypted_replay_round_trips_through_the_file`, `jrn_3_a_foreign_schema_epoch_is_refused_before_records`, `jrn_4_create_append_reopen_and_immediate_visibility`, `runtime_clock_values_reach_session_and_turn_chronology`, `a_default_session_is_durable_and_ephemeral_is_an_explicit_opt_out` |
| JRN-4 | `jrn_4_create_append_reopen_and_immediate_visibility`, `jrn_4_a_second_writer_is_refused_until_the_owner_closes`, `jrn_4_valid_final_record_without_newline_is_repaired`, `jrn_4_incomplete_final_tail_is_isolated`, `jrn_4_middle_corruption_is_not_guessed_around`, `jrn_4_write_failure_changes_no_memory_and_requires_reopen`, `jrn_4_partial_write_reopens_at_the_last_complete_record`, `jrn_4_newline_write_failure_recovers_the_record_as_committed`, `jrn_4_oversized_record_is_returned_without_poisoning_the_writer`, `jrn_4_failed_header_encoding_leaves_no_file`, `jrn_4_rejected_append_writes_nothing_and_returns_exact_ownership`, `jrn_4_unterminated_line_cannot_grow_past_the_bound_when_repaired`, `jrn_4_poisoned_writer_cannot_fork`, `jrn_4_fork_publishes_a_complete_sibling`, `jrn_3_and_jrn_4_journal_fork_and_tail_files_are_owner_only`, `jrn_3_and_jrn_4_insecure_existing_journal_is_refused`, `jrn_3_and_jrn_4_encrypted_replay_round_trips_through_the_file`, `maximal_valid_assistant_output_fits_the_journal_line_envelope`, `jrn_4_session_paths_stay_inside_an_owner_only_directory`, `jrn_4_symlinked_session_directory_and_file_are_refused`, `jrn_4_automatic_session_identity_is_portable_uuid_v7`, `jrn_4_automatic_session_names_retry_uuid_collisions`, `a_default_session_is_durable_and_ephemeral_is_an_explicit_opt_out`, `failed_fresh_runtime_construction_removes_its_unusable_session`, `a_torn_final_record_resumes_with_one_typed_visible_recovery` |
| JRN-5 | `jrn_5_canonical_live_turn_and_journal_replay_have_equal_model_context`, `jrn_5_one_path_projects_model_order_and_visible_lifecycle`, `jrn_5_head_mutations_refuse_the_first_parallel_result_boundary`, `tool_batch_accepts_only_model_call_order`, `prv_1_both_protocols_preserve_parallel_call_and_result_order`, `jrn_5_incomplete_tool_batch_is_explicit_and_absent_from_the_request`, `jrn_5_incomplete_tool_batch_before_later_content_is_rejected`, `jrn_5_named_heads_project_only_their_selected_ancestry`, `jrn_5_hidden_replay_and_visible_diagnostics_project_to_their_exact_consumers`, `jrn_5_invalid_tool_lifecycle_has_a_typed_projection_error`, `jrn_5_duplicate_transcript_identity_is_rejected_before_projection`, `jrn_5_mail_requires_both_visible_endpoints`, `jrn_5_attention_resolution_keeps_its_request_owner`, `jrn_5_journal_projection_builds_the_model_request_and_tui_state`, `abort_discards_complete_but_undispatched_stream_calls`, `recovery_completes_calls_declared_before_their_request_record`, `an_unfinished_restored_turn_becomes_idle_with_a_stable_cancelled_tool_result`, `an_atomic_turn_start_is_recovered_as_interrupted`, `process_recovery_is_idempotent_across_every_record_prefix`, `process_death_has_one_stable_provider_tool_result`, `a_real_created_session_resumes_with_equal_visible_and_model_projections`, `an_unfinished_final_turn_resumes_once_as_interrupted_without_an_effect`, `the_session_recovery_frames_match_their_fixtures`, crate-graph gate |
| JRN-6 | `jrn_6_one_commit_is_the_only_model_record`, `jrn_5_canonical_live_turn_and_journal_replay_have_equal_model_context`, `jrn_5_multi_delta_live_turn_and_replay_have_equal_visible_semantics`, `jrn_6_active_projection_rebuild_is_refused`, `jrn_6_reused_tool_call_identity_fails_the_turn_before_commit`, `jrn_6_partial_failure_and_output_limit_keep_live_transcript_order`, `jrn_6_interleaved_answer_and_reasoning_keep_first_open_order`, `jrn_6_streaming_usage_warning_keeps_live_transcript_order`, `decreasing_provider_output_positions_fail_before_entering_canonical_order`, `reverse_parallel_call_completion_is_sorted_before_dispatch` |
| JRN-7 | `durable_transition_starts_no_effect_before_append_ack`, `tim_2_agent_authorizes_only_the_exact_active_step_without_advancing_context`, `tool_effects_start_only_after_their_transition_is_acknowledged`, `failed_claim_of_queued_next_turn_text_returns_the_exact_input`, `failed_claim_of_queued_steering_returns_the_exact_input`, `a_burst_stops_after_journal_failure_and_yields_the_queued_input_report`, `cancelled_submit_keeps_commit_owned_until_next_poll`, `cancelled_shutdown_drains_every_accepted_record_before_writer_exit`, `submit_after_cancelled_shutdown_returns_text_without_a_record_or_effect`, `failed_shutdown_returns_interleaved_queued_input_and_joins_the_model`, `failed_user_append_returns_the_draft_and_starts_no_effect`, `uncertain_user_append_is_typed_and_cannot_start_an_effect`, `failed_atomic_turn_start_is_wholly_unwritten`, `panicked_writer_returns_input_and_reports_failed_cleanup`, `the_persistence_failure_frames_match_their_fixtures`, crate-graph gate |
