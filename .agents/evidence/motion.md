# Evidence — Motion

What proves [motion](../specs/motion.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| MOT-1 | `launch_greets_once_and_leaves`, `the_first_message_ends_the_greeting_for_good`, `a_conversation_launch_did_not_greet_shows_no_mark`, `mot_1_one_deadline_serves_every_mover_and_late_wakes_coalesce`, `the_mark_cycle_divides_the_clock_cycle`, `effort_xhigh_labels_remain_static_without_an_animation_deadline`, `effort_animation_changes_only_visible_max_cells_and_stops_when_hidden` |
| MOT-2 | `the_mark_is_braille_one_cell_a_glyph`, `mot_2_every_activity_mark_frame_is_one_cell`, `effort_animation_changes_only_visible_max_cells_and_stops_when_hidden` |
| MOT-3 | `effort_animation_changes_only_visible_max_cells_and_stops_when_hidden` |

## Rendered

Launch's greeting over a new conversation, drawn by the workspace on its motion clock with
`cargo run -p plexmaton-tui --example greeting_preview -- <directory>`: the centre grown into a
circle at [120](../../crates/plexmaton-tui/frames/greeting/greeting-circle-120.svg), [88](../../crates/plexmaton-tui/frames/greeting/greeting-circle-88.svg) and
[60](../../crates/plexmaton-tui/frames/greeting/greeting-circle-60.svg), and centre and frame turned at [120](../../crates/plexmaton-tui/frames/greeting/greeting-turned-120.svg),
[88](../../crates/plexmaton-tui/frames/greeting/greeting-turned-88.svg) and [60](../../crates/plexmaton-tui/frames/greeting/greeting-turned-60.svg). The user watched it play in
the real binary in kitty on 2026-09-23; `cargo run -p plexmaton-tui --example mark_greeting` plays
it alone in any terminal.
