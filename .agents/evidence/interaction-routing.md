# Evidence — Interaction routing

What proves [interaction-routing](../specs/interaction-routing.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| INV-1, INV-11 | `retry_click_keyboard_and_drag_cancellation_share_one_action`, `retry_frames_keep_actions_with_the_failed_request_at_three_widths` with the `retry-*` frames, `a_press_on_one_surface_cannot_activate_a_release_on_another`, `a_press_a_drag_inside_the_same_row_and_a_release_activate_nothing`, `narrow_agents_handle_and_rows_require_a_matching_release` |
| INV-1 | `every_terminal_event_is_translated_or_named_as_ignored` |
| INV-2 | `approval_numbers_follow_focus_and_the_current_choice_stage`, `printable_keys_follow_the_cursor`, `the_inspector_grammar_is_the_same_under_both_focus_modes_except_enter`, `ctrl_o_is_the_same_disclosure_intent_under_both_focus_modes` |
| INV-3 | `approval_hover_and_arrows_share_selection_and_repeated_motion_is_free`, `drawer_hover_and_arrows_share_one_choice_without_opening_pages`, `composer_menu_hover_preserves_draft_and_arrows_continue_from_the_hovered_row`, `pointer_motion_routes_a_hover_without_capture_or_focus`, `hover_changes_only_the_foldable_rows_appearance_and_repeating_it_costs_nothing`, `wheel_routes_by_hover_and_never_changes_focus`, `the_wheel_falls_through_what_cannot_scroll_and_stops_at_what_is_merely_exhausted`, `a_wheel_over_the_workspace_with_nothing_to_scroll_says_so` |
| INV-4 | `capture_is_released_exactly_once`, `capture_keeps_the_drag_on_its_surface`, `wheel_is_not_captured_by_a_drag`, `focus_loss_suspends_motion_without_releasing_capture`, `dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle` |
| INV-6 | `escape_resolves_one_layer_per_press`, `selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation`, `narrow_agents_navigation_restores_focus_and_commits_only_on_enter`, `drawer_owns_ctrl_b_without_corrupting_narrow_agents_return_focus`, `drawer_and_tree_own_ctrl_b_while_they_cover_the_workspace` |
| INV-7 | `quit_is_explicit_and_unreachable_while_typing`, `the_quit_chord_confirms_only_inside_its_one_second_window`, `the_quit_deadline_expires_once_and_costs_one_frame`, `ctrl_c_clears_a_draft_or_interrupts_but_never_does_both`, `ctrl_c_names_the_conversation_it_interrupts`, `production_mapping_preserves_message_steering_interrupt_and_approval`, `ctrl_c_child_refusal_never_falls_back_to_root_and_root_remains_routable`; `scripts/smoke-delegate.py` proves focused-child Stop does not exit or interrupt Main |
| INV-8 | `a_modifier_does_not_make_a_pointer_event_disappear`, `dragging_across_a_conversation_selects_and_copies_what_it_crossed` |
| INV-9 | `resize_is_an_intent` |
| INV-10 | `an_arrow_moves_the_strip_and_scrolls_everything_else`, `an_arrow_moves_the_roster_and_entering_an_asking_agent_goes_to_its_request`, `approval_keys_stay_inside_the_blocking_surface`, `narrow_agents_has_one_closed_navigation_grammar` |
