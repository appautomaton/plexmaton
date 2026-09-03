# Spec — Live runtime

| Field | Value |
| --- | --- |
| Status | Implemented through Phase 01 stage 2 slice 10 |
| Owns | Live agent ownership, model-step correlation, native-tool scheduling, cancellation, reported token usage, and the executable-to-agent composition boundary |
| Depends on | [agent-loop](./agent-loop.md), [tool-admission](./tool-admission.md), [provider-adapter](./provider-adapter.md), [workspace-files](./workspace-files.md), [command-tool](./command-tool.md), [frame-loop](./frame-loop.md) |
| Proven by | `plexmaton-runtime` component and opt-in live tests, provider fixtures, agent correlation tests, CLI startup test, and TUI reducer test |

## Invariants

**LIVE-1 — One runtime owns one live agent and all its work.** The runtime accepts addressed user
input, drives `Agent::handle`, publishes its `SessionEvent` envelopes, and performs effects; the
provider is one retained future rather than a detached task, and every native admission or
execution runs on a bounded per-call worker the runtime joins. `Drop` cancels and joins those
workers and drops the provider future; orderly shutdown remains the semantic transition. One
catalog advertises four workspace-file definitions and one command definition through either
codec; native outcomes are bounded before replay. The TUI knows only semantic events and intents.

**LIVE-2 — Every model event names the step that requested it.** A stable typed step identity
travels on `CallModel`, streamed input and model failure. Only the current matching step accepts
output; stale, repeated or post-cancellation output is a typed non-delivery.

**LIVE-3 — Interrupt and shutdown cancel the same owned operation.** The agent transition first
settles its semantic debt, then the runtime signals every matching provider, admission and
execution operation and drives each to completion. One outcome wins a completion/cancellation
race; cancelled polls retain owned work; shutdown starts no new requests and joins before terminal
restoration. A cancelled shutdown call resumes retained cleanup when called again.

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
never searches a project `.plexmaton/`. It also canonicalizes the process's current directory as
the sole native workspace, resolves `rg` only from absolute `PATH` entries, and pins the current
executable plus a fixed private argument as the directory-search driver. Any failure happens before
entering the alternate screen; the selected key variable's exact name reaches the native catalog
so commands exclude it even when it has no credential-shaped suffix. Credentials, encrypted
reasoning and authorization headers have redacted diagnostics.

## Model

```text
TUI intent ─▶ CLI route ─▶ LiveRuntime ─▶ Agent::handle ─▶ SessionEvent ─▶ TUI
                                ▲              │
                                │         CallModel(step)
                   provider future ◀───────────┤
                                │         AdmitTool / RunTool
                                │              └────▶ retained native future
                                │                           │
                                └── typed completion ◀──────┘
```

One user turn may issue several model steps. Usage belongs to a step; the turn total is their
checked sum, with coverage saying whether that sum is complete.
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
| UI closes during admission or tool execution | Shutdown cancels and joins every retained worker before terminal restoration |
| A shutdown poll is itself cancelled | The shutdown state and work remain owned; the next call resumes cleanup |
| Runtime is dropped without shutdown | Drop cancels and joins native workers and drops the provider future; it claims no semantic completion |
| A native result exceeds its final bound | A bounded typed failure enters the record; JSON is never silently truncated |
| Current directory, `rg` or internal driver cannot be pinned | Typed startup failure before terminal initialization |

## Evidence

| Invariant | Proven by |
| --- | --- |
| LIVE-1 | `native_catalog_is_exact_unique_and_advertised_by_both_protocols`, `file_observation_survives_the_runtime_boundary_into_an_approved_edit`, `maximal_command_result_stays_bounded_in_the_next_model_request`, `dropping_an_active_runtime_drops_the_exact_provider_future`, `dropping_an_active_runtime_joins_its_command_worker_and_process_group`, `a_native_tool_round_trip_is_a_stream_the_projection_accepts`, `real_model_completes_read_observed_edit_and_command_with_exact_approvals`, `scripts/smoke-tui.py --live`, crate-graph gate |
| LIVE-2 | `stale_and_post_cancellation_model_output_is_a_typed_non_delivery`, `cancellation_wins_a_queued_completion_race_without_touching_a_later_turn` |
| LIVE-3 | `interrupt_and_shutdown_cancel_and_join_the_exact_provider_task`, `a_cancelled_terminal_join_remains_owned_until_interrupt_joins_it`, `cancelled_next_event_keeps_command_work_owned_until_interrupt_joins_it`, `cancelled_shutdown_can_be_called_again_to_finish_exact_cleanup`, `cancellation_wins_a_queued_completion_race_without_touching_a_later_turn` |
| LIVE-4 | `prv_1_chat_fixture_drives_a_full_stateless_tool_round_trip`, `prv_3_responses_fixture_replays_encrypted_reasoning_exactly_and_round_trips_tools`, `a_combined_chat_terminal_chunk_orders_usage_before_stop`, `reported_step_usage_is_aggregated_for_the_owning_turn`, `usage_is_retained_without_charging_an_invisible_frame`, `real_model_completes_read_observed_edit_and_command_with_exact_approvals` |
| LIVE-5 | `missing_step_usage_is_never_presented_as_zero`, `responses_null_usage_breakdowns_are_partial_coverage`, `deterministic_failure_paths_leave_no_provider_task_alive`, `interrupt_and_shutdown_cancel_and_join_the_exact_provider_task` |
| LIVE-6 | `prv_6_resolves_only_an_override_or_the_user_root`, `prv_6_key_resolution_is_explicit_and_redacted`, `cmd_2_selected_api_key_environment_is_removed_even_without_credential_shape`, `catalog_key_identity_must_match_profile`, `endpoint_resolution_is_protocol_specific_and_rejects_embedded_authority`, `invalid_configuration_never_takes_over_the_terminal`, `executable_private_driver_reenters_a_pinned_directory_and_execs_ripgrep` |
