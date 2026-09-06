# Spec — Session timing

| Field | Value |
| --- | --- |
| Status | Implemented; compaction execution follows CPL-6/CPL-7 |
| Owns | Durable turn chronology, model-request attempt timing and immutable provider usage |
| Depends on | JRN-1/JRN-5/JRN-7, LOOP-1/LOOP-4/LOOP-6, PRV-1/PRV-5 and LIVE-1/LIVE-3/LIVE-4/LIVE-5 |
| Proven by | Agent/journal accounting tests, JSONL reopen tests and hermetic runtime/HTTP tests below |

## Invariants

**TIM-1 — User and turn time name semantic boundaries.** Every claimed user item retains its
accepted wall time; an initial item atomically starts a turn with stable item/turn identities, exact
text and opened wall time. One terminal fact records its typed outcome and either known completion
wall time or separately named recovery-observed wall time. These boundary facts are the durable
lifecycle authority and project the starting/terminal agent status. Idle emits no time record;
sequence and ancestry, never time, decide order.
An explicit retry starts a separately timed execution through `TurnRetried` without another user
item (JRN-8); prior turn and request accounting remain unchanged.

Intra-turn status facts may still project current work, but cannot independently start or finish a
turn; no separate terminal status append follows `TurnFinished`. Recovery of every valid prefix is
therefore idempotent around both boundaries.

**TIM-2 — Dispatch authorization and measured execution are distinct.** A durable pre-effect fact
names the attempt and its pre-append authorization wall time; JRN-7 acknowledges it before network
work. The adapter samples wall and monotonic clocks immediately before HTTP dispatch. One immutable
terminal fact says whether dispatch occurred and retains measurements only when it did; process
death may leave only authorization. Rejected: treating journal latency as API latency.

**TIM-3 — Request accounting is immutable and correlated.** A stable request-attempt identity links
one typed owner, exact semantic-prefix boundary, request-environment fingerprint, provider/model,
terminal outcome, exact provider-reported usage and incurred cost fixed under the model pricing
resolved for that attempt. The fingerprint covers instructions and tools without copying them into
the journal. Missing usage or pricing remains unavailable and a retry is distinct.
Turn totals fold only reachable agent-step attempts; whole-session incurred usage folds every
unique agent-step and compaction attempt once, never once per head.

**TIM-4 — Timing cannot perturb model context or cache ancestry.** Canonical audit facts correlate
to a semantic path but neither advance its head nor project as a `ContextAtom`. Equal semantic
ancestry and request environment encode byte-identically with or without timing inspection. A
checkpoint begins a cache epoch; rewind and head selection only select an existing identity. Audit
record IDs, journal sequence, wall time and head revision are excluded from that identity.

**TIM-5 — Partial timing stays honest.** Cancellation and typed model failure end an owned attempt
as not-dispatched or dispatched according to the effect boundary. Process death may leave durable
authorization without a terminal attempt; recovery reports it as outcome unknown and never
fabricates dispatch, duration or usage.
Provider-declared errors retain `ProviderFailed`, separately from `TransportFailed` and
`Malformed`; PRV-5 owns classification and JRN-8 owns retry eligibility.

## Request timing state

```text
Authorized { authorized_at, semantic_boundary, request_environment }
    ├── NotDispatched { outcome }
    └── Dispatched {
          dispatched_at,
          headers_after_ms?,
          first_output_after_ms?,
          terminal_after_ms,
          outcome,
          usage,
          cost
        }
```

An attempt owner is `AgentStep { step_id }` or `Compaction { compaction_id }`; those typed
identities already carry their turn or operation, while authorization names the source boundary.
Compaction attempts have their own usage total and
enter whole-session incurred cost, but never inflate a turn or serve as an agent-request usage
anchor. CPL-6 owns its collected output and CPL-7 owns execution.

`Authorized.authorized_at` is observed before its record is appended; it is not a claim about when
the append was acknowledged. The fact also records the semantic-prefix boundary and
request-environment fingerprint; its terminal fact refers only to that identity. `NotDispatched`
is restricted to cancellation or typed preparation/encoding failure before `.send()`.
`TurnUsageUpdated` is only a UI projection of attempts. A streamed `ModelEvent::Usage` sent directly
to the agent is a typed refusal; the runtime retains provider usage in the terminal audit.

The request environment fingerprint is SHA-256 over a versioned, length-delimited structural
encoding of replay owner, codec identity/revision, model family, reasoning effort, the exact
optional output limit, explicit instruction text, cache intent and ordered tool name, description and
canonical JSON Schema. Codec revision owns fixed wire flags. Display name, token budget/reserve, compaction retention,
estimator and pricing do not change request bytes and are excluded. Provider credentials are not
available at this boundary. Rejected: process-random hashing and serialized map insertion order,
which cannot identify equal request environments across resume.

All dispatched offsets are integer milliseconds from one adapter-owned `Instant` sampled
immediately before `.send()`. Present milestones satisfy `headers ≤ first_output ≤ terminal`.
Headers may be absent only when no response arrived; first output may be absent. First output means
the first non-empty text or reasoning delta, complete tool call, or opaque replay item; usage and
stop markers do not count. Milestones accumulate only in the owned request task and enter the
journal together in its terminal fact, never as streaming records. The current transport has no
typed timeout outcome, so this spec does not invent one under `ModelError::Transport`.

Dispatched cost is either unavailable or a nonnegative fixed-point USD amount at 10^10 ticks per
dollar; newly measured cost requires protocol completion, reported input/output and both input-cache
categories, while the optional reasoning split is not independently priced. Cancelled or failed
requests retain observed usage but leave cost unavailable: even a complete field breakdown can be
an interim snapshot. `NotDispatched` incurs zero. The terminal stores the
resolved amount so later pricing changes cannot rewrite historical session cost; journal types use
no floating point. Session and compaction totals are on-demand folds over unique attempt identities;
shared ancestry and abandoned heads never multiply a charge. Any unknown contribution keeps the
cost unavailable, and integer overflow is typed. An unresolved attempt makes surviving turn counts
partial; a late terminal can resolve that coverage without fabricating a measurement.

Accepted time travels process-locally with queued next-turn input and next-step steering and becomes
durable only when the matching LOOP-6 boundary claims it. It is retained on both initial and
steering user items; queueing alone creates no journal fact.

Durable wall observations use validated `UnixMillis(u64)` and monotonic offsets use checked
`ElapsedMillis(u64)`; both serialize as integer milliseconds. The owned composition/runtime samples
wall time through an injected clock, and the HTTP adapter owns each request `Instant`. Agent,
journal reduction and replay receive those values and never read a clock, so deterministic tests
use a fake source and loading cannot invent new chronology.

Wall values establish chronology only and are never subtracted to claim turn, tool or provider
duration; elapsed values come only from one monotonic `Instant`. Tool/approval timing is outside
these slices: call-to-result wall distance includes admission, approval, scheduling and ordered
finalization. Provider response/trace IDs are deferred diagnostics and, if later retained under
PRV-4, never become recovery or cache identity.

## Journal and tree placement

The initial user/turn fact and claimed steering are semantic entries and advance one checked head.
`TurnFinished`, request authorization and request terminal facts are typed `JournalRecord`s outside
the `ConversationEntry` tree. They advance only the global journal sequence and correlate through stable
turn, step, attempt and semantic-boundary identities; head rename cannot change their ownership.
For one selected head, a projector admits audit facts only when their owning `TurnId` starts on its
path and their semantic boundary is on that path. Whole-session cost instead folds every unique
attempt once.

`CreateHead` and `MoveHead` may target only a stable semantic boundary: no `TurnStarted` on its path
lacks a canonical `TurnFinished`, and no context atom is split. Checked semantic appends are the
owned live-progress exception; abandoning a partial live head is refused, while rename preserves
its ownership. Rewinding to a user item moves the new head to the boundary before that turn and
returns the exact text as a user-owned draft; explicit resubmission creates a fresh global
`TurnId`. Exit before resubmission cannot look like crashed work. Two heads extended from one
completed boundary own distinct later turns and attempts without a speculative `BranchId`.

## Evidence

| Invariant | Proven by |
| --- | --- |
| TIM-1 | `tim_1_turn_boundaries_are_durable_and_terminal_time_does_not_advance_the_head`, `tim_1_queued_turn_and_steering_keep_their_original_accepted_time`, `tim_1_every_live_turn_terminal_path_has_a_typed_outcome`, `cancelled_submit_behind_an_older_commit_keeps_its_arrival_time_and_text`, `tim_1_turn_chronology_reopens_from_jsonl_without_entering_model_context`, `tim_1_jsonl_rejects_untimed_turns_and_unscoped_lifecycle_records` |
| TIM-2 | `tim_2_agent_authorizes_only_the_exact_active_step_without_advancing_context`, `model_dispatch_waits_for_its_request_authorization_ack`, `model_terminal_audit_commits_before_semantic_completion`, `pre_dispatch_outcomes_have_no_request_measurements_or_signals`, `dispatched_response_returns_one_correlated_terminal_report`, `dispatched_http_failures_preserve_their_terminal_measurements`, `first_output_distinguishes_semantic_content_from_usage_and_stop`, `tim_2_invalid_milestone_order_is_refused_by_constructor_and_wire`, `tim_2_invalid_attempt_records_change_nothing` |
| TIM-3 | `completed_messages_without_thinking_breakdown_keep_final_cost`, `native_stream_cancellation_preserves_observed_usage`, `malformed_stream_after_usage_preserves_reported_consumption`, `reported_step_usage_is_aggregated_for_the_owning_turn`, `tim_3_streamed_usage_is_refused_without_mutating_agent_state`, `tim_3_terminal_cost_is_immutable_validated_and_non_floating_point`, `tim_3_complete_usage_calculates_one_stable_fixed_point_cost`, `tim_3_cost_does_not_require_the_optional_reasoning_breakdown`, `tim_3_priced_request_with_missing_reasoning_breakdown_round_trips`, `tim_3_request_affecting_route_model_and_tool_inputs_break_the_fingerprint`, `tim_3_session_accounting_counts_shared_attempts_once`, `tim_3_session_accounting_includes_abandoned_branches_and_compaction`, `tim_3_accounting_keeps_partial_missing_and_unpriced_attempts_honest`, `tim_3_accounting_rejects_usage_and_cost_overflow`, `request_attempts_reopen_with_identical_accounting_and_context`, `model_output_requires_both_the_active_attempt_and_step` |
| TIM-4 | `tim_2_attempt_records_are_lossless_and_non_advancing`, `tim_2_agent_authorizes_only_the_exact_active_step_without_advancing_context`, `tim_1_sibling_heads_project_only_their_own_later_turns_and_terminals`, `tim_3_equal_environment_inputs_have_one_canonical_fingerprint`, `tim_4_non_request_model_metadata_preserves_the_fingerprint`, `request_attempts_reopen_with_identical_accounting_and_context`, `checkpoint_preserves_history_and_refreshes_the_active_step`, `cpl_5_repeated_checkpoints_and_historical_forks_reopen_with_identical_wire_bytes` |
| TIM-5 | `gemini_declared_failures_preserve_diagnostics_without_dispatching_calls`, `stream_provider_errors_keep_their_category_and_observed_usage`, `provider_failure_reopens_as_the_same_non_retryable_outcome`, `tim_5_terminal_after_interrupt_is_retained_and_invalid_terminals_mutate_nothing`, `tim_5_not_dispatched_terminal_restores_known_turn_coverage_after_interrupt`, `tim_5_late_terminal_keeps_other_unresolved_step_usage_partial`, `dropped_runtime_reopens_authorization_without_a_fabricated_terminal`, `cancelled_model_end_during_attempt_terminal_append_keeps_the_active_owner`, `cancellation_after_dispatch_keeps_only_observed_milestones`, `cancellation_after_output_preserves_only_observed_usage`, `malformed_stream_after_usage_preserves_reported_consumption`, `failed_request_authorization_starts_no_model_and_freezes_the_runtime`, `failed_request_terminal_publishes_no_semantic_completion` |
