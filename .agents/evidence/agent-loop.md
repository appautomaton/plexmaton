# Evidence — Agent loop

What proves [agent-loop](../specs/agent-loop.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| LOOP-1 | `a_turn_stops_at_its_step_budget_and_says_so` |
| LOOP-2 | `an_interrupt_leaves_a_result_for_every_call_it_dispatched`, `shutdown_pays_what_dispatched_calls_owe` |
| LOOP-3 | `results_are_assembled_in_the_order_the_model_asked_and_not_the_order_they_finished` |
| LOOP-4 | `an_interrupted_turn_keeps_what_arrived_and_leaves_no_item_open`, `an_interrupt_starts_no_new_work_and_returns_what_was_waiting`, `reasoning_and_opaque_replay_survive_interrupt_without_sharing_presentation` |
| LOOP-5 | `a_protected_call_waits_as_state_and_allow_once_resumes_that_exact_call`, `deny_pays_the_call_debt_and_a_duplicate_decision_is_typed`, `interrupt_and_shutdown_cancel_pending_approval_as_explicit_state` |
| LOOP-6 | `steering_is_claimed_only_by_the_current_turns_next_step`, `input_without_its_boundary_is_returned_with_its_text_intact`, `failure_and_budget_return_pending_steering`, `agent_returns_the_exact_input_that_overflows_its_queue`, `production_mapping_preserves_message_steering_interrupt_and_approval`, `a_live_dispatch_restores_undelivered_user_text`, `tim_1_queued_turn_and_steering_keep_their_original_accepted_time`, `cancelled_submit_behind_an_older_commit_keeps_its_arrival_time_and_text`, `submit_after_cancelled_shutdown_returns_text_without_a_record_or_effect` |
