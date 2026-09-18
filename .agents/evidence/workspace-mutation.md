# Evidence — Workspace mutation

What proves [workspace-mutation](../specs/workspace-mutation.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| MUT-1 | `exact_batch_preserves_byte_shape_and_mode`, `admission_enforces_the_observed_window_and_unique_target` |
| MUT-2 | `mutation_paths_pin_the_parent_and_keep_the_leaf_separate`, `mutation_paths_refuse_symlinked_parents_and_leaves`, `execution_rechecks_revision_and_capabilities` |
| MUT-3 | `stale_edit_preserves_the_concurrent_writer`, `writer_before_replace_publication_is_preserved`, `symlink_swap_before_replace_is_refused` |
| MUT-4 | `replacement_preserves_unix_mode_matrix`, `exact_batch_preserves_byte_shape_and_mode`, `replacement_faults_leave_no_partial_target_or_staging_file`, `replaced_or_modified_staging_entry_is_never_published_or_wrongly_deleted`, `malformed_canonical_and_cancelled_mutations_fail_closed` |
| MUT-5 | `create_is_absence_only_and_never_overwrites`, `create_faults_and_collision_never_publish_staging_bytes`, `mutation_paths_allow_a_missing_leaf_but_not_a_missing_parent` |
| MUT-6 | `mutation_bounds_hold_at_their_exact_edges`, `escaped_edit_within_raw_and_mutation_bounds_survives_canonicalization`, `maximum_edit_canonical_structure_fits_the_one_kibibyte_reserve`, `maximum_valid_edit_retains_a_complete_bounded_patch`, `admission_never_returns_a_trusted_call_after_final_cancellation`, `catalog_never_publishes_a_trusted_call_after_final_cancellation`, `malformed_canonical_and_cancelled_mutations_fail_closed`, `replacement_faults_leave_no_partial_target_or_staging_file`, `create_faults_and_collision_never_publish_staging_bytes` |
