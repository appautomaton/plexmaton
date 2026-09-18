# Evidence — Personal project permission store

What proves [project-permissions](../specs/project-permissions.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Evidence |
| --- | --- |
| PGR-1 | `pgr_1_aliases_share_identity_but_distinct_and_replaced_roots_do_not`, `pgr_1_private_permissions_and_pinned_parents_reject_replacement`, `pgr_1_symlinks_hardlinks_and_replaced_locks_supply_no_authority` |
| PGR-2 | `pgr_2_two_process_writers_commit_once_and_never_merge_stale_grants`, `pgr_2_stale_writes_and_reset_cannot_recreate_an_old_revision`, `pgr_2_dispatch_after_another_process_revokes_observes_the_revoke`, `pgr_2_a_process_waits_for_authorization_lock_then_observes_current_policy`, `pgr_2_cancelled_wait_and_mutation_apply_nothing`, `pgr_2_process_death_releases_the_stable_lock_and_torn_writes_stay_refused`, `per_6_project_command_survives_restart_and_dispatch_observes_external_revoke` |
| PGR-3 | `pgr_3_torn_complete_without_newline_and_invalid_records_never_restore_a_prefix`, `pgr_3_format_binding_and_resource_bounds_refuse_the_whole_source`, `pgr_3_active_grant_and_retained_mutation_limits_do_not_partially_apply`, `per_4_permission_wire_scopes_validate_the_same_constructors`, `per_6_corrupt_project_source_refuses_allow_once_before_its_effect` |
| PGR-4 | `pgr_4_failed_partial_and_unknown_writes_publish_no_success`, `pgr_4_lost_grant_ack_retains_the_durable_grant_without_reporting_success`, `per_6_project_grant_saved_then_conversation_audit_failed_starts_no_effect` |
| PGR-5 | `pgr_5_absence_grants_trust_and_revocation_have_one_personal_source`, `per_8_project_allow_requires_exact_personal_trust_and_edits_invalidate_the_review`, `per_8_trusted_configuration_runs_the_command_and_dispatch_rechecks_changed_bytes` |
