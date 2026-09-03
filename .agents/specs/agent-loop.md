# Spec — Agent loop

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Turn and step lifecycle, input-boundary routing, tool-call debt and approval state |
| Depends on | [UI/UX](../ui-ux.md) §state matrix for queued and undelivered steering |
| Proven by | `plexmaton-agent::turn` tests and the executable's boundary tests |

## Invariants

**LOOP-1 — A turn is one or more budgeted steps.** A step is one model request and the tool calls
it returns. Exhausting the step budget is a typed, visible failure rather than a silent stop.

**LOOP-2 — Every dispatched tool call owes a result.** A call the loop dispatched is answered even
when the turn fails or is interrupted; an interrupted call is answered as aborted.

**LOOP-3 — Results are model-ordered.** Calls whose effects permit it may finish concurrently, but
the batch shown to the model is assembled in the order the model emitted it.

**LOOP-4 — A turn is one value.** Its lifecycle state, provider replay metadata and outstanding
debt are inspectable state rather than a stack frame, callback or adapter private.

Rejected: a turn as one `async fn`, and cancellation as an error propagated for each caller to
interpret; neither leaves one state that can be rendered, resumed and tested between inputs.

**LOOP-5 — Approval is state, not a suspended call.** A call requiring approval parks the turn in
a pending record; a typed command resolves that record and resumes that exact call.

Rejected: an approval callback returning a future, which cannot itself be counted, rendered or
resumed.

**LOOP-6 — User input names the boundary it means.** A submitted message waits for the next turn;
steering waits for the current turn's next step. The loop claims it only while opening that
boundary, and returns an unclaimable input with its exact text and a typed reason.

Rejected: one queue drained at whichever boundary happens first, which silently turns steering
into a later conversation or a new message into part of a turn already in flight.

## Model

```text
Submitted ──▶ next-turn queue ──▶ turn boundary ──▶ user item + step 1
Steered   ──▶ next-step queue ──▶ step boundary ──▶ user item + next step
                     │
                     └─ no matching boundary ──▶ UndeliveredInput(text, reason)
```

## Evidence

| Invariant | Proven by |
| --- | --- |
| LOOP-1 | `a_turn_stops_at_its_step_budget_and_says_so` |
| LOOP-2 | `an_interrupt_leaves_a_result_for_every_call_it_dispatched`, `shutdown_pays_what_dispatched_calls_owe` |
| LOOP-3 | `results_are_assembled_in_the_order_the_model_asked_and_not_the_order_they_finished` |
| LOOP-4 | `an_interrupted_turn_keeps_what_arrived_and_leaves_no_item_open`, `an_interrupt_starts_no_new_work_and_returns_what_was_waiting`, `reasoning_and_opaque_replay_survive_interrupt_without_sharing_presentation` |
| LOOP-5 | `a_protected_call_waits_as_state_and_allow_once_resumes_that_exact_call`, `deny_pays_the_call_debt_and_a_duplicate_decision_is_typed`, `interrupt_and_shutdown_cancel_pending_approval_as_explicit_state` |
| LOOP-6 | `steering_is_claimed_only_by_the_current_turns_next_step`, `input_without_its_boundary_is_returned_with_its_text_intact`, `failure_and_budget_return_pending_steering`, `agent_returns_the_exact_input_that_overflows_its_queue`, `production_mapping_preserves_message_steering_interrupt_and_approval`, `a_live_dispatch_restores_undelivered_user_text` |
