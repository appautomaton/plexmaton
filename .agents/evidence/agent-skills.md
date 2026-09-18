# Evidence — Agent Skills

What proves [agent-skills](../specs/agent-skills.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| SKL-1 | `project_selection_overrides_only_the_user_active_model`, `project_config_rejects_authority_and_unrecognized_fields`, `project_config_refuses_symlinks_at_every_component`, `linked_worktree_uses_its_checkout_instead_of_the_common_repo` |
| SKL-2 | `collisions_select_one_origin_and_missing_winners_do_not_fall_back`, `metadata_is_strict_typed_and_unicode_normalized`, `candidate_frontmatter_catalog_and_content_bounds_are_typed`, `model_skill_call_records_real_read_while_catalog_omits_body` |
| SKL-3 | `bounded_reads_are_pinned_no_follow_and_cancelled_before_return`, `bounded_read_reports_completion_without_retaining_the_probe_byte`, `resources_are_exact_confined_and_recheck_invocation_after_replacement`, `derived_skill_root_symlinks_never_load_external_metadata`, `directory_listing_is_bounded_and_cancellable`, `cancellation_precedes_discovery_and_reads` |
| SKL-4 | `explicit_user_only_skill_allows_resource_but_not_unprompted_body`, `unavailable_explicit_skills_restore_input_without_model_dispatch` |
| SKL-5 | `cpl_3_skill_invocation_survives_compaction_in_every_dialect`, `skl_5_explicit_submission_records_skill_separately_before_dispatch`, `skl_5_queued_turn_and_steering_retain_skill_activation`, `skl_5_skill_activation_rejects_wrong_ownership_and_order_without_mutation`, `explicit_skill_context_survives_source_deletion_and_jsonl_resume`, `edited_retry_prepares_skill_before_replacing_the_failed_branch` |
| SKL-6 | `skill_preparation_completes_while_compaction_is_waiting`, `skl_6_every_codec_budgets_exact_skill_wire_content`, `skill_context_is_exact_and_uses_supported_user_roles_in_every_dialect`, `skill_preparation_cannot_starve_interrupt_at_input_capacity`, `skill_preparation_preserves_shutdown_and_persistence_failure_causes`, `the_skill_diagnostic_frames_match_their_fixtures` |
