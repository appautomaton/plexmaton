# Spec — Live runtime

| Field | Value |
| --- | --- |
| Status | Implemented in Phase 01 stage 2 slice 6 |
| Owns | Live agent task ownership, model-step correlation, cancellation, reported token usage, and the executable-to-agent composition boundary |
| Depends on | [agent-loop](./agent-loop.md) LOOP-1, LOOP-4 and LOOP-6; [provider-adapter](./provider-adapter.md) PRV-1, PRV-5 through PRV-7; [frame-loop](./frame-loop.md) FR-1 |
| Proven by | `plexmaton-runtime` component and opt-in live tests, provider fixtures, agent correlation tests, CLI startup test, and TUI reducer test |

## Invariants

**LIVE-1 — One runtime owns one live agent and all its work.** The runtime accepts addressed user
input, drives `Agent::handle`, publishes its `SessionEvent` envelopes, and performs effects; every
provider task and bounded channel has that runtime as owner, an observable terminal result, and a
join path. The TUI knows only semantic events and intents, while the provider knows no runtime or
projection.

**LIVE-2 — Every model event names the step that requested it.** A stable typed step identity
travels on `CallModel`, streamed input and model failure. The agent accepts output only for its
currently open matching step; stale, repeated or post-cancellation output is a typed non-delivery
and can never attach to a later turn.

**LIVE-3 — Interrupt and shutdown cancel the same owned operation.** The agent transition first
settles its semantic debt, then the runtime signals the matching provider task and awaits its end.
Exactly one terminal outcome wins a completion/cancellation race, no new request starts during
shutdown, and dropping the TUI never detaches network work.

**LIVE-4 — Reported usage is exact, step-scoped and turn-aggregated.** A completed provider stream
emits exactly one `TokenUsage` before its stop; Chat requests streaming usage explicitly and
Responses reads it from the terminal response. Counts retain input, cached input, cache-write input,
output, reasoning output and the provider's total without recomputing subsets; checked addition
produces the turn total across tool-loop steps.

**LIVE-5 — Missing usage is not zero.** A provider omission, transport failure or cancellation is
an explicit coverage state (`Complete`, `Partial` or `Unavailable`) beside any reported counts.
Reported usage measures completed consumption only; pre-request context estimation and monetary
pricing are separate mechanisms and never inferred from it.

**LIVE-6 — Configuration is resolved before terminal or network ownership.** The composition root
uses `PLEXMATON_HOME` or the user-level default and reads the chosen key environment variable; it
never searches a project `.plexmaton/`. Invalid configuration fails before entering the alternate
screen, and credentials, encrypted reasoning and authorization headers have redacted diagnostics.

## Model

```text
TUI intent ─▶ CLI route ─▶ LiveRuntime ─▶ Agent::handle
                                │              │
                         owned task ◀── CallModel(step)
                                │
                  Streamed/Failed(step) + TokenUsage
                                │
                                └─▶ Agent ─▶ SessionEvent ─▶ TUI
```

One user turn may issue several model steps once tools exist. Provider usage belongs to a step;
the turn total is the checked sum of reports, with coverage saying whether that sum is complete.
Cached input remains a subset of input and reasoning remains a subset of output.

## Failure modes

| Situation | Response |
| --- | --- |
| Config or key is absent | Typed startup failure before terminal initialization |
| HTTP request or SSE decode fails | Matching step receives one typed failure and the task terminates |
| `Ctrl-C` races with the final SSE event | One terminal transition wins; the other is stale by step identity |
| A cancelled stream returns a late delta | Typed non-delivery; no record, frame or later turn changes |
| Provider omits usage | Answer remains valid and usage coverage is unavailable, never zero |
| One usage field or turn sum overflows | Malformed report; no wrapped or saturated count is displayed |
| UI closes during a request | Shutdown cancels and joins the request before terminal restoration completes |

## Evidence

| Invariant | Proven by |
| --- | --- |
| LIVE-1 | `sequential_turns_stream_and_report_their_own_usage`, `one_local_request_streams_text_and_reported_usage`, `scripts/smoke-tui.py --live`, crate-graph gate |
| LIVE-2 | `stale_and_post_cancellation_model_output_is_a_typed_non_delivery`, `cancellation_wins_a_queued_completion_race_without_touching_a_later_turn` |
| LIVE-3 | `interrupt_and_shutdown_cancel_and_join_the_exact_provider_task`, `a_cancelled_terminal_join_remains_owned_until_interrupt_joins_it`, `cancellation_wins_a_queued_completion_race_without_touching_a_later_turn`, `deterministic_failure_paths_leave_no_provider_task_alive` |
| LIVE-4 | `prv_1_chat_fixture_drives_a_full_stateless_tool_round_trip`, `prv_3_responses_fixture_replays_encrypted_reasoning_exactly_and_round_trips_tools`, `a_combined_chat_terminal_chunk_orders_usage_before_stop`, `reported_step_usage_is_aggregated_for_the_owning_turn`, `usage_is_retained_without_charging_an_invisible_frame` |
| LIVE-5 | `missing_step_usage_is_never_presented_as_zero`, `responses_null_usage_breakdowns_are_partial_coverage`, `deterministic_failure_paths_leave_no_provider_task_alive`, `interrupt_and_shutdown_cancel_and_join_the_exact_provider_task` |
| LIVE-6 | `prv_6_resolves_only_an_override_or_the_user_root`, `prv_6_key_resolution_is_explicit_and_redacted`, `endpoint_resolution_is_protocol_specific_and_rejects_embedded_authority`, `invalid_configuration_never_takes_over_the_terminal` |
