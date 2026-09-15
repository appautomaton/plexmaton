# Spec — Owned collaboration scheduling

| Field | Value |
| --- | --- |
| Status | Implemented and wired. `scripts/smoke-delegate.py` accepts owned Stop beside a responsive root; the post-Handoff User input lane is accepted at tier 2 and reaches the product UI in stage 7.6 |
| Owns | Asynchronous collaboration-file ownership, bounded child-runner lanes, scheduling authority and joined stop/Handoff |
| Depends on | COL-3/COL-4; CIN-4; CHB-1–CHB-3; LIVE-1/LIVE-3 |
| Proven by | Runtime tests named below |

## Invariants

**SCH-1 — One asynchronous owner crosses each mutable boundary.** One joined worker exclusively owns
the blocking `CollaborationFile`; one supervised task exclusively owns each `LiveRuntime`. Callers
use bounded typed commands and replies and cannot obtain mutable file/runtime access. Main
scheduling and User input occupy distinct one-slot lanes. Cancelling a reply wait does not cancel
an accepted command or discard its retained result, including regular admission, User input and
Handoff before preflight acknowledgement.

**SCH-2 — Control retains reserved progress.** Normal scheduling, User input, runtime updates,
disposable inspection and control use separate bounded lanes. Saturated normal, User or update
traffic cannot prevent stop or shutdown admission; inspection consumes no control capacity. Stop
admission is synchronous and owner-retained: repeating the same target is idempotent, while a
different target receives one typed in-progress refusal. Accepted User input settles before Stop;
new input cannot enter while Stop is pending, and shutdown retains its exact settlement and draft.
An update names the runner endpoint and process-local generation; output from a retired generation
cannot settle its replacement, including across replacement owner instances. Unexpected runner
termination is one terminal typed owner update emitted after cleanup output; observing a terminal
update also joins that runner, so root activity cannot spin on a finished, unjoined slot.

**SCH-3 — Scheduled execution owns authority exactly once.** The owner checks explicit provider
capability and reserves bounded runner capacity before durable admission. The accepted command carries one resolved admission plus its
non-cloneable reservation and ticket; it binds the permit before child-session inclusion and retains
it through queued/model/tool work, cancellation and join. Generic admission cannot bypass the owned
turn or Handoff paths.

**SCH-4 — Stop and Handoff join all child ownership.** Stop interrupts the addressed runner and
retains its report until accepted work and its permit settle. A schedule already accepted for that
child settles first and returns beside the Stop report; Stop removes that child's queued wake before
control admission, so late activity cannot restart it. Handoff first validates the canonical mutation,
closes normal and regular admission, settles queued and active work, joins any terminal runner whose
observation was cancelled, and only then appends the durable Handoff.
Only then may an owner-issued process-local target activate an exact runner-generation ticket and
admit User input to that canonical live runner; reopen requires explicit cold activation of the
existing delegated journal and rebuilds Handoff-closed Main wake state without dispatch. An exact
durable Handoff retry returns its receipt without stopping User-owned work.
Shutdown resumes internally retained schedule/User-input/Stop/Handoff operations without a caller
token, returns their exact results, joins every runner and then the writer even when quiescence fails.
Dropping a runner aborts its task rather than
detaching it; orderly product teardown uses joined shutdown. Idle, completion, surface closure and
reply cancellation never transfer control.

**SCH-5 — Wake is advisory and canonical.** One content-free hint names an exact live runner
generation and coalesces per child under the runner bound. An idle child supplies a fresh
branch-local boundary and cursor; the owner then rereads canonical eligible facts through CIN-4 and
uses the existing SCH-3 schedule path. Busy and cancelled work retains the exact obligation, while
stale generations, unsupported providers, Stop, Handoff and shutdown refuse or discard it before a
new admission. Registration restores the selected branch's prior resolved collaboration context
before accepting a resumed child. Wake is never restored automatically after process restart.

## Evidence

| Invariant | Proven by |
| --- | --- |
| SCH-1 | `sch_1_writer_finishes_accepted_admission_after_reply_cancellation`, `sch_1_worker_panic_retains_attempt_and_sticky_failure`, `sch_1_writer_channel_is_bounded_while_the_owner_is_blocked`, `sch_1_cancelled_owner_schedule_resumes_the_exact_accepted_command`, `sch_1_cancelled_regular_admission_retains_its_completion`, `sch_1_cancelled_handoff_preflight_resumes_the_owned_attempt`, `sch_1_cancelled_cold_handoff_has_one_typed_owner_settlement`, `sch_2_cancelled_user_input_wait_retains_one_settlement_and_exact_backpressure`, `sch_4_shutdown_returns_a_cancelled_regular_admission_result`, `sch_4_actor_panic_joins_active_provider_and_releases_authority` |
| SCH-2 | `sch_2_stop_completes_while_the_update_lane_is_saturated`, `ctl_1_child_stop_does_not_deadlock_behind_its_accepted_mail`, `cmp_2_active_owned_child_projects_session_mail_without_blocking_stop`, `sch_2_owner_stop_initiation_is_cancellation_safe`, `sch_2_owner_stop_initiation_is_idempotent_and_exclusive`, `sch_2_owner_stop_on_idle_removes_wake_and_refuses_unknown`, `sch_2_owner_multiplexes_two_independent_runners`, `sch_2_runner_generation_does_not_restart_with_a_new_owner`, `sch_2_cancelled_terminal_join_resumes_before_terminal_publication`, `sch_2_cancelled_user_input_wait_retains_one_settlement_and_exact_backpressure`, `sch_2_cross_child_stop_bypasses_an_unrelated_blocked_user_input`, `sch_4_failed_runtime_emits_one_terminal_marker_after_cleanup`, `sch_4_owner_surfaces_runner_panic_instead_of_clean_stream_closure`, `sch_4_owner_schedules_and_hands_off_only_after_child_quiescence`; `scripts/smoke-delegate.py` proves focused-child Stop and root continuation |
| SCH-3 | `sch_3_generic_admission_refuses_turns_without_mutating_the_file`, `sch_3_schedule_preflights_authority_before_turn_admission`, `sch_3_prepared_execution_retains_authority_until_disposed`, `sch_3_provider_refusal_precedes_collaboration_admission`, `sch_1_cancelled_owner_schedule_resumes_the_exact_accepted_command`, `sch_4_shutdown_settles_a_cancelled_schedule_without_its_request_token` |
| SCH-4 | `sch_2_stop_completes_while_the_update_lane_is_saturated`, `ctl_1_child_stop_does_not_deadlock_behind_its_accepted_mail`, `sch_2_cancelled_terminal_join_resumes_before_terminal_publication`, `sch_2_cancelled_user_input_wait_retains_one_settlement_and_exact_backpressure`, `sch_2_cross_child_stop_bypasses_an_unrelated_blocked_user_input`, `sch_4_begin_stop_repoll_preserves_pending_schedule_report`, `sch_4_failed_user_input_still_stops_active_work_and_returns_the_exact_request`, `sch_1_cancelled_cold_handoff_has_one_typed_owner_settlement`, `sch_4_cold_handoff_waits_for_a_cancelled_terminal_join`, `stop_settlement_is_consumed_exactly_once`, `stop_settlement_restores_exact_child_input_without_a_new_event`, `resumed_child_stop_refusal_is_repeatable_and_root_is_untouched`, `sch_4_failed_runtime_emits_one_terminal_marker_after_cleanup`, `sch_4_handle_drop_cooperatively_releases_the_child_writer`, `sch_4_active_handle_drop_aborts_without_detaching_runtime_authority`, `sch_4_actor_panic_joins_active_provider_and_releases_authority`, `sch_4_owner_surfaces_runner_panic_instead_of_clean_stream_closure`, `sch_4_writer_joins_after_a_poisoned_quiescence_check`, `sch_4_owner_schedules_and_hands_off_only_after_child_quiescence`, `sch_4_shutdown_returns_a_cancelled_regular_admission_result`, `sch_4_shutdown_settles_a_cancelled_schedule_without_its_request_token`, `sch_4_shutdown_settles_a_cancelled_stop_without_caller_replay`, `sch_4_shutdown_settles_a_cancelled_handoff_without_caller_replay`, `sch_4_handoff_preflight_precedes_stop_and_retains_its_report`, `col_3_handoff_unlocks_only_the_authenticated_owned_child_input`, `col_3_idle_handoff_opens_user_input_until_owned_stop_begins`, `col_3_reopened_user_control_requires_explicit_activation_and_preserves_history`; `scripts/smoke-delegate.py` proves no late child output after Stop |
| SCH-5 | `sch_5_wake_coalesces_and_rereads_the_canonical_prefix`, `ctl_1_main_mail_wakes_an_engaged_child_for_a_fresh_second_turn`, `sch_5_wake_rejects_a_stale_runner_generation_without_mutation`, `sch_5_cancelled_wake_resumes_without_a_second_admission`, `sch_5_busy_child_runs_one_retained_wake_after_quiescence`, `sch_5_stop_in_progress_preserves_another_child_wake`, `sch_5_writer_busy_wake_does_not_spin_or_starve_updates`, `sch_5_wake_admission_identity_collision_never_dispatches`, `sch_5_resumed_child_restores_context_before_wake_dispatch`, `sch_3_provider_refusal_precedes_collaboration_admission`, `sch_4_owner_schedules_and_hands_off_only_after_child_quiescence` |

## Integration boundary

This mechanism ends at exact scheduling requests, typed runtime updates and durable collaboration
facts. Stage 6 owns provider mail representation and [CMP-1](./collaboration-mail-projection.md);
Stage 7
owns native tools and product composition, including authenticating the Main ingress that holds the
owner capability. The scheduler adds no Tokio or I/O ownership to `plexmaton-agent` and does not use
provider text as an authority channel.
