# Evidence — Child control view

What proves [child-control-view](../specs/child-control-view.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| CCV-1 | `ccv_1_snapshots_refuse_wrong_targets_stale_and_conflicting_revisions`, `ccv_1_handoff_activity_projects_pending_before_acknowledgement`, `ccv_1_failed_handoff_projection_rolls_forward_to_main_before_retry`, `control_snapshots_apply_to_the_announced_child_and_unlock_after_handoff` |
| CCV-2 | `ccv_2_non_user_children_have_no_input_cursor_or_submission`, `ccv_2_control_survives_dismissal_reopen_and_resize`, `ccv_2_user_child_input_uses_its_lifecycle_without_primary_commands`, `ccv_2_primary_collapse_matches_visible_child_input_across_short_heights`, `production_handoff_routes_child_input_and_retains_the_locked_draft`; `scripts/smoke-delegate.py` |
| CCV-3 | `ccv_3_control_chrome_stays_outside_the_transcript_at_three_widths`, `ccv_3_handoff_entry_is_distinct_at_three_widths`, `handoff_projects_distinct_entries_on_both_sides`; `scripts/smoke-delegate.py` |
| CCV-4 | `ccv_4_acknowledgment_is_passive_and_preserves_reading_state`, `ccv_4_interrupt_preserves_hidden_input_and_control`, `ccv_4_control_loss_settles_input_drag_without_copy_or_hidden_escape`, `ccv_4_hidden_input_release_settles_and_escape_closes_the_window`, `control_snapshots_apply_to_the_announced_child_and_unlock_after_handoff` |
