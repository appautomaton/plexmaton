# Evidence — Collaboration turn inclusion

What proves [collaboration-inclusion](../specs/collaboration-inclusion.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


`scripts/smoke-delegate.py` kills the actual CLI after acknowledged task/mail inclusion, during a
paused child request and before pending Handoff admission. Exact collaboration bytes and normalized
task, control, correspondence and admission projections survive passive and repeated resume with no
redispatch; one explicit root continuation receives the same canonical task/mail context.

| Invariant | Proven by |
| --- | --- |
| CIN-1 | `cin_1_admission_orders_updates_and_handoff_and_freezes_original_revision`, `cin_1_source_capacity_holds_the_turn_without_advancing_log`, `cin_1_source_bytes_include_attributed_agent_identities`, `cin_2_reopen_between_logs_keeps_unincluded_items_pending` |
| CIN-2 | `cin_2_automatic_journal_materializes_first_collaboration_turn`, `cin_2_unincluded_admission_and_branch_retain_pending_sources`, `cin_2_foreign_and_duplicate_references_fail_closed`, `cin_3_session_reference_resolves_without_synthetic_user_content`, `collaboration_link_refuses_a_foreign_session_agent`, `durable_links_place_shared_rows_and_legacy_rows_keep_a_stable_suffix`, `selected_session_placement_ignores_an_off_branch_foreign_link`, `selected_session_placement_rejects_a_link_from_another_announced_agent`, `orphaned_root_ingress_persists_one_link_before_live_projection`, `cin_2_reopen_between_logs_keeps_unincluded_items_pending`, `cin_4_uncertain_inclusion_reopens_without_redispatch` |
| CIN-3 | `cin_3_session_reference_resolves_without_synthetic_user_content`, `cin_3_resolved_cache_is_bounded_and_exact_reinsertion_is_free`, `cin_3_resolved_cache_byte_cap_is_independent_of_turn_count`, `cin_3_every_codec_renders_collaboration_with_its_sender_named`, `cin_3_unsupported_driver_refuses_before_session_mutation` |
| CIN-4 | `cin_4_inclusion_and_request_authorization_each_gate_dispatch`, `cin_4_cancelled_start_retains_inclusion_until_acknowledgement`, `cin_4_uncertain_inclusion_reopens_without_redispatch`, `cin_4_unresolved_history_never_strands_an_authorized_step` |
