# Evidence — The second window

What proves [inspector](../specs/inspector.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| INS-1 | `a_projection_reset_keeps_the_open_window_and_its_presentation`, `a_working_child_keeps_the_window_the_user_opened_on_it`, `the_strip_is_registered_only_when_there_is_a_roster`, `the_window_floats_over_the_primary_and_escape_closes_it`, `clicking_an_agent_in_the_list_selects_it_and_opens_its_window`, `narrow_agents_navigation_restores_focus_and_commits_only_on_enter`, `narrow_agents_handle_and_rows_require_a_matching_release`, `the_journey_reaches_two_agents_without_losing_the_first`, `the_journey_keeps_a_second_agent_on_screen_and_takes_a_request_without_being_interrupted`; `scripts/smoke-delegate.py` opens the same resumed child by pointer and keyboard |
| INS-2 | `an_open_inspector_leaves_ten_readable_rows_or_takes_the_region_outright`, `a_dragged_height_is_clamped_rather_than_obeyed`, `dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle` |
| INS-3 | `presentation_follows_the_terminal_and_the_users_maximize`, `the_composer_survives_every_presentation`, `registered_surfaces_tile_the_terminal_without_gaps_or_overlap`, `the_presentation_survives_the_window_showing_another_agent` |
| INS-4 | `selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation`, `the_inspector_grammar_is_the_same_under_both_focus_modes_except_enter`, `narrow_agents_navigation_restores_focus_and_commits_only_on_enter`, `agents_handle_and_resize_transition_follow_the_layout_boundary`; `scripts/smoke-delegate.py` enters the resumed read-only child and closes it back to Main at 60 columns |
| INS-5 | `the_inspector_takes_the_cursor_and_the_composer_keeps_one_row`, `only_a_press_on_the_bottom_edge_starts_a_resize`, `the_keyboard_moves_the_inspectors_edge_the_same_way_the_pointer_does`, `a_wheel_over_the_inspector_input_scrolls_that_inspectors_conversation`; CCV-2 |
| INS-6 | `two_conversations_scroll_independently_and_neither_moves_the_other`, `an_inspected_conversation_keeps_its_own_reading_position_across_a_close_and_reopen`, `the_journey_reaches_two_agents_without_losing_the_first`, `a_conversation_drawn_at_two_widths_measures_correctly_at_both`, `missing_resumed_child_history_projects_one_explicit_unavailable_state`, `locked_resumed_child_history_projects_one_explicit_unavailable_state`, `corrupt_resumed_child_history_projects_one_explicit_unavailable_state`; `scripts/smoke-delegate.py` shows the exact restored child entries at all three widths, retains a semantic first-visible line through close/reopen and changes no durable bytes |
| INS-7 | `an_inspector_too_short_for_its_input_takes_no_typing_and_no_cursor`, `an_inspector_splits_for_its_input_only_when_both_still_fit` |
| INS-8 | `resizing_a_maximized_inspector_preserves_the_shelf_height`, `derived_maximized_and_column_inspectors_have_no_resize_edge` |
