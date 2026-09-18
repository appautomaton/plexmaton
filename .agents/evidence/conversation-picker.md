# Evidence — Conversation picker

What proves [conversation-picker](../specs/conversation-picker.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| SPK-1 | `a_wrapped_status_says_its_whole_sentence_and_reserves_the_rows_it_draws`, `resume_lists_saved_conversations_by_identity_for_keyboard_and_pointer` with the `composer-menu-resume-loading-medium` and `composer-menu-resume-medium` frames, `resume_status_rows_cover_failure_no_match_and_opening`, `listing_is_bounded_read_only_and_rejects_symlinks` |
| SPK-2 | `a_switch_that_stops_a_working_child_asks_on_the_row_the_quit_chord_uses`, `a_switch_past_a_working_child_is_offered_once_and_only_the_same_choice_takes_it`, `only_a_child_with_work_to_lose_is_offered_for_a_switch`, `session_switch_validates_before_replacing_and_never_dispatches`, `cancelled_picker_releases_candidate_and_preserves_current_draft`, `new_session_is_lazy_and_replacement_preserves_saved_history`, `new_session_refuses_unsent_input_and_active_work`, `continuation_handoff_names_only_the_selected_saved_session`, `a_refused_switch_is_a_note_and_the_listing_offers_its_rows_again` |
| SPK-4 | `switching_carries_a_live_collaboration_into_the_replacement`, `an_ephemeral_replacement_has_no_collaboration_and_no_delegate_tool` |
| SPK-3 | `cancelled_picker_releases_candidate_and_preserves_current_draft`, `new_session_is_lazy_and_replacement_preserves_saved_history`, `a_refused_switch_is_a_note_and_the_listing_offers_its_rows_again`. Listing afresh after a withdrawal is unproven |
