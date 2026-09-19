# Evidence — Waiting input

What proves [input-queue](../specs/input-queue.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| IQU-1 | `a_message_typed_mid_turn_is_reported_until_its_boundary_claims_it`, `completed_skill_input_wakes_the_waiting_projection_without_a_model_delta`, `input_held_by_an_owned_operation_is_reported_and_taken_back_newest_first`, `every_boundary_names_itself_once_above_its_own_entries`, `waiting_input_is_reported_above_the_composer_at_every_width` with the `input-queue-*` frames |
| IQU-2 | `the_waiting_band_yields_to_both_inputs_and_to_a_readable_conversation`, `height_is_bounded_by_what_is_listed_rather_than_by_what_is_waiting`, `a_multiline_message_occupies_one_row_without_joining_its_lines`, `a_sending_time_the_band_stopped_listing_is_counted_rather_than_named`, `a_band_cut_short_drops_messages_before_it_drops_the_way_back`, `a_registered_band_shows_every_waiting_message_and_the_way_back`, `waiting_input_is_reported_above_the_composer_at_every_width` |
| IQU-3 | `chrome_is_neither_a_pointer_target_nor_a_focus_stop`, `every_registered_surface_is_drawn_inside_its_own_bounds`, `alt_up_takes_back_the_last_waiting_message_and_a_bare_arrow_still_moves_the_caret`, `the_way_back_is_inert_until_something_waits_and_then_names_the_primary` |
| IQU-4 | `the_newest_waiting_message_comes_back_with_its_exact_text`, `input_held_by_an_owned_operation_is_reported_and_taken_back_newest_first`, `taking_a_message_back_leaves_the_running_turn_alone`, `waiting_input_keeps_an_existing_draft_and_its_skill_separate`, `the_way_back_is_inert_until_something_waits_and_then_names_the_primary`, `a_live_dispatch_restores_undelivered_user_text` for the composer it lands in |

Reviewed frames: [wide](../../crates/plexmaton-tui/frames/input-queue-wide.txt),
[medium](../../crates/plexmaton-tui/frames/input-queue-medium.txt),
[narrow](../../crates/plexmaton-tui/frames/input-queue-narrow.txt).

The [executable journey](../../scripts/smoke-input-queue.py) proves IQU-1/IQU-4 with a paused
loopback stream and real `Alt-↑` bytes: occupied-draft refusal, exact multiline text and numeric
skill binding, unchanged journal bytes during withdrawal, continued first response, and the
remaining queued message before explicit resubmission. It captures actual single-agent frames
at 120/88/60 columns. The fixture uses three local requests and no live credentials or model.
