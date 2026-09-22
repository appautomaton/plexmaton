# Evidence — Motion

What proves [motion](../specs/motion.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| MOT-1 | `mot_1_one_deadline_serves_every_mover_and_late_wakes_coalesce`, `effort_xhigh_labels_remain_static_without_an_animation_deadline`, `effort_animation_changes_only_visible_max_cells_and_stops_when_hidden` |
| MOT-2 | Unproven beyond `effort_animation_changes_only_visible_max_cells_and_stops_when_hidden`, which checks the effort markers' width |
| MOT-3 | `effort_animation_changes_only_visible_max_cells_and_stops_when_hidden` |
