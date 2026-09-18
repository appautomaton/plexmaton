# Evidence — Reasoning effort

What proves [reasoning-effort](../specs/reasoning-effort.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| EFF-1 | `effort_replacement_updates_wire_and_environment_without_rewriting_history`, `effort_changes_refuse_active_and_queued_work`, `effort_command_changes_the_live_driver_without_submitting_a_message` |
| EFF-2 | `effort_hover_previews_without_applying_and_ignores_disabled_stops`; `effort_selection_confirms_only_after_runtime_acceptance_and_escape_cancels`, `effort_pointer_ignores_disabled_stops_and_drag_disarms_selection`, `effort_provider_default_does_not_preselect_an_explicit_level`; EFF-1's production loop test |
| EFF-3 | `effort_animation_changes_only_visible_max_cells_and_stops_when_hidden`, `effort_xhigh_labels_remain_static_without_an_animation_deadline`; user-reviewed precursor, followed by requested vertical-only ticks and removal of the pentagon |
| EFF-4 | `effort_animation_changes_only_visible_max_cells_and_stops_when_hidden`, `effort_xhigh_labels_remain_static_without_an_animation_deadline`; physical-terminal/font fidelity remains unproven |
| EFF-5 | `effort_command_changes_the_live_driver_without_submitting_a_message`; `model_override_expires_on_new_resume_and_restart` and `scripts/smoke-model.py` prove reset-on-replacement |
