# Spec — Tool admission and approval

| Field | Value |
| --- | --- |
| Status | Implemented through Phase 01 stage 2 slice 10 |
| Owns | How an untrusted model tool request becomes an admitted call, how policy decides it, and how an approval decision names it |
| Depends on | [agent-loop](./agent-loop.md) LOOP-2 through LOOP-5; [attention](./attention.md) ATT-1 and ATT-3; Phase 01 §producer |
| Proven by | `plexmaton-agent::{admission,tools,turn}`, native-tool admission, `plexmaton-runtime::runtime::tests::tools`, and `plexmaton-tui::{approval,workspace,frames}` tests |

## Invariants

**APV-1 — Authority starts at admission.** The model supplies an untrusted tool name and raw
arguments. The trusted tool catalog validates and canonicalizes them through an explicit loop
effect carrying a non-cloneable, loop-issued `AdmissionRequest`. Consuming that request produces
one immutable admitted call or a typed refusal before policy or execution can run; neither the
model, a presentation adapter, nor a holder of an already admitted call can construct another one.

**APV-2 — Policy reads capabilities, not names.** An admitted call carries its tool-definition
identity and revision, normalized arguments, a finite set of typed capabilities, and bounded
decision detail. Policy returns exactly `Allow`, `RequireApproval`, or `Forbidden`; tool names,
display labels and prompt prose are never authority. The first live policy allows `FileRead`,
requires approval for `FileWrite` or `ProcessSpawn`, and forbids no capability by default.

**APV-3 — Approval grants permission, not validity.** `AllowOnce` authorizes only the admitted call
the request pins. It cannot override an admission refusal, relax workspace confinement, satisfy an
integrity precondition such as read-before-edit, or replace enforcement at the executor boundary.

**APV-4 — A decision names one stable pending call.** A pending record binds an `ApprovalId` to its
`AgentId`, `TurnId`, `ToolCallId` and admitted call. The UI returns that ID with only `AllowOnce` or
`Deny`; an absent, stale or mismatched ID is a typed non-decision and the UI never repeats policy
matching.

**APV-5 — Waiting is per call.** A protected call may wait while admitted siblings run; the model
does not receive the batch until every slot has paid its result debt, assembled in model order
(LOOP-2 and LOOP-3). Denial and cancellation finish the exact slot with typed results rather than
discarding its payload.

**APV-6 — Waiting has no hidden waiter and no approval timeout.** The pending record is inspectable
turn state (LOOP-4 and LOOP-5), not a task, callback, channel sender or blocking thread. Interrupt,
turn cancellation and shutdown explicitly cancel it; a future durable restore must rerun admission
and current policy instead of trusting a recorded approval request.

## Model

```text
Requested
    │
    ▼
AwaitingAdmission ── refusal ─────────────────────────▶ Finished(refused)
    │ admitted call
    ▼
Policy ── Forbidden ──────────────────────────────────▶ Finished(forbidden)
    ├──── Allow ──────────────────────────────────────▶ Running ──▶ Finished(result)
    └──── RequireApproval ──▶ AwaitingApproval
                                  ├─ AllowOnce ───────▶ Running
                                  ├─ Deny ────────────▶ Finished(denied)
                                  └─ cancel/shutdown ─▶ Finished(cancelled)
```

Each tool call owns one slot. The batch owns their model order; completion order is not a second
ordering.

## Extension boundaries

| Boundary | Current | Later |
| --- | --- | --- |
| Tool catalog | Native read, search, create, edit and command definitions declare schemas, capabilities and bounded details | Phase 02 adds MCP definitions behind the same admission boundary |
| Approval policy | Stateless policy over the admitted call; reads run, while writes and process spawning ask once | Phase 02 may add session- and workspace-scoped grants behind a durable policy store |
| Decision vocabulary | `AllowOnce`, `Deny` | A stored grant is a policy operation, not an `ApproveAll` answer smuggled through the UI |
| Presentation | Attention requested and resolved are projections of loop state | The user-reviewed decision surface may vary by transport without owning pending state |
| Executor | The live runtime sends only admitted, allowed calls to filesystem and process adapters, which enforce their own hard constraints | Network and MCP adapters enter through the same boundary |

## Failure modes

| Situation | Response |
| --- | --- |
| Unknown tool or malformed arguments | Typed admission refusal; no approval request and no execution |
| Policy forbids a valid call | Typed forbidden result; approval cannot be requested to bypass it |
| Approval transport is absent | The call remains pending; never a fail-open default |
| A decision arrives after denial, cancellation or completion | Typed stale decision; no slot changes |
| The turn is interrupted or the runtime shuts down while waiting | The slot is cancelled and its result debt is paid before the turn closes |
| Allowed siblings finish while another waits | Their results remain in their slots and reach the model only when the ordered batch is complete |
| A persisted pending request is restored in Phase 02 | Its recorded decision is not authority; admission and policy run again against current definitions and rules |

## Evidence

| Invariant | Proven by |
| --- | --- |
| APV-1 | `admitted_state_is_bounded_before_the_loop_can_retain_it`, `forbidden_and_admission_refusal_finish_without_approval_or_execution`, `cmd_1_admission_is_strict_canonical_and_pins_the_workspace`, `cmd_1_refuses_every_shape_outside_the_model_contract_and_hard_bounds` |
| APV-2 | `capabilities_are_a_canonical_set`, `policy_uses_capabilities_and_forbidden_wins`, `cmd_1_admission_is_strict_canonical_and_pins_the_workspace`, `cmd_1_approval_detail_keeps_root_and_command_head_tail_separately`, `denied_command_has_no_side_effect_and_keeps_model_order_with_a_read_sibling` |
| APV-3 | `forbidden_and_admission_refusal_finish_without_approval_or_execution`, `file_observation_survives_the_runtime_boundary_into_an_approved_edit`, `real_model_completes_read_observed_edit_and_command_with_exact_approvals`, `stale_edit_preserves_the_concurrent_writer`, `malformed_canonical_and_cancelled_mutations_fail_closed`, `cmd_1_executor_refuses_a_call_pinned_to_another_workspace` |
| APV-4 | `a_protected_call_waits_as_state_and_allow_once_resumes_that_exact_call`, `deny_pays_the_call_debt_and_a_duplicate_decision_is_typed`, `a_decision_echoes_the_open_request_and_cannot_recompute_policy`, `production_mapping_preserves_message_steering_interrupt_and_approval`, `real_model_completes_read_observed_edit_and_command_with_exact_approvals`, `the_native_approval_frames_match_their_fixtures` |
| APV-5 | `a_safe_sibling_runs_while_a_protected_call_waits_and_results_keep_model_order`, `results_are_assembled_in_the_order_the_model_asked_and_not_the_order_they_finished`, `denied_command_has_no_side_effect_and_keeps_model_order_with_a_read_sibling` |
| APV-6 | `interrupt_and_shutdown_cancel_pending_approval_as_explicit_state`, `abandoning_answers_everything_outstanding_and_leaves_settled_calls_alone`; durable restore remains unproven until Phase 02 |
