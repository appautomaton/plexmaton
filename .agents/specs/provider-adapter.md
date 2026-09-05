# Spec — Provider adapter

| Field | Value |
| --- | --- |
| Status | Implemented; four dialects verified with hermetic tests |
| Owns | Configuration, request encoding, streaming decode, reasoning replay, and typed failure at the model-provider boundary |
| Depends on | [agent-loop](./agent-loop.md) LOOP-1, LOOP-3 and LOOP-4 |
| Proven by | `plexmaton-provider::{config,codec,sse}` tests, sanitized protocol fixtures, `plexmaton-runtime::http` tests, and `plexmaton-agent::turn` tests |

## Invariants

**PRV-1 — Wire dialects end at one semantic port.** Responses, Chat Completions, Messages and
GenerateContent encode the same ordered `ContextAtom`s and produce position-bearing semantic model events; neither owns a loop,
transcript, tool scheduler or approval path. API selection is explicit and never changes
mid-turn.

**PRV-2 — Streaming assembly is bounded and finality is explicit.** Text deltas pass through in
order; tool identity and argument fragments assemble under per-call, aggregate-step and count bounds,
and only a protocol completion event emits one whole call. Partial UTF-8, an unfinished call,
duplicate call identity, duplicate finality or a conflicting fragment is a typed malformed
response, never guessed content.
The production raw-call ceiling remains 64 KiB even when trusted canonical state reserves more;
a semantic stop is released only after the stream trailer passes validation.

**PRV-3 — Semantics and replay are exact but separate.** Every exposed reasoning artifact is
retained in output order. Bounded replay sidecars attach to text, reasoning or calls; a replay-only
block retains a part with no visible content. Responses retains item identity/status, message phase
and content grouping as well as encrypted reasoning. Messages retains complete thinking/signature and redacted
blocks; Gemini retains signatures on their original parts, signature-only text-field absence and
optional upstream call IDs. Chat
retains recognized reasoning field identity and refuses unsupported structured reasoning.
Opaque replay carries adapter-owned route owner, codec revision and model family compatibility; mismatch is typed. It is
never rendered, copied, logged, truncated or treated as semantic text; exceeding its bound fails
the step visibly. Unsigned reasoning outside tool batches stays in the journal and presentation;
signature-requiring wire encoders omit it without fabricating replay.

**PRV-4 — The semantic record is replay authority.** Every request is rebuilt from the record,
including exact provider replay sidecars and tool-call identities. The same selected journal path,
resolved configuration, ordered tool schemas and output cap produce the same wire request. Gemini local call IDs are scoped
to the request attempt; only retained upstream IDs enter the wire call/result pair. A response-level
ID is diagnostic metadata only: the selected proxy reports `store: false`, and an unavailable `previous_response_id`
must not strand the session or create an adapter-private transcript.

**PRV-5 — Stops and failures stay typed.** Each protocol maps its declared completion, output-limit,
refusal, rate-limit, context-limit, provider failure, transport and malformed states without matching presentation text.
An HTTP rejection or declared stream error without a more specific category is `ProviderFailed`;
`Transport` means the response could not arrive intact, and `Malformed` means decoding failed.
Unknown additive wire events are observable and ignored only when they carry no semantic content;
an unknown content-bearing event fails rather than silently losing output.

**PRV-6 — Configuration names data, never authority.** `~/.plexmaton/config.toml` separates named
provider routes from their named models and selects one exact provider/model pair. A route owns its
base URL, credential environment and default API; a model owns its wire/display identity, optional
API override, optional reasoning controls, stable instructions, cache intent, context/output/reserve
limits, estimator and optional price
snapshot. Resolution yields one immutable credential-blind value: omitted estimators become an
explicit versioned default, while omitted pricing remains unavailable. Selection never uses fuzzy
names or URL inference. `PLEXMATON_HOME` redirects the whole root for isolated development; keys
never enter the file, diagnostics, repository or a native command's environment, and invalid input
fails before network work begins. Rejected: a combined provider/model profile, inline keys, and
project-local `.plexmaton/` discovery; project corpus belongs in `.agents/`.

**PRV-7 — Local bounds do not trust upstream hints.** Provider token limits may be forwarded but
are not memory or context boundaries. The stream owner bounds retained output and replay locally,
has one cancellation path, and reports truncation/cancellation explicitly; the selected proxy's
handling of a token hint is not a security invariant.

## Model

```text
ModelRequest (session identity + atoms) + resolved model + tool schemas
                  │
        explicit protocol codec
    ┌─────────────┼─────────────┬────────────────┐
    │             │             │                │
 Responses       Chat        Messages     GenerateContent
    └─────────────┴─────────────┴────────────────┘
                  ▼
      ModelEvent + ProviderReplay
```

The development route is an OpenAI-compatible proxy at `http://127.0.0.1:8317/v1`; its `luna`
model uses `gpt-5.6-luna` over Responses, while Chat Completions remains an explicit model API
choice. Rejected: xAI as the first provider, because it is not the endpoint Plexmaton's local
development loop exercises. Rejected: automatic fallback, because replay and failure semantics
change across APIs.

## Failure modes

| Situation | Response |
| --- | --- |
| Missing config, provider/model selection or key environment variable | Typed configuration error before transport construction |
| Tool arguments exceed their bound or the stream ends mid-call | Malformed step; no partial call reaches admission |
| Responses continuation cannot resolve a response ID | Rebuild stateless input from the authoritative semantic record and exact replay items |
| Encrypted reasoning exceeds its bound | Fail the step; never truncate an opaque replay capsule |
| Stream is cancelled or disconnects | One terminal typed outcome; no detached reader remains |
| Effort is outside the selected dialect's grammar | Typed configuration error before transport construction; no silent downgrade |
| Provider rejects a model-specific option or reports a server error | `ProviderFailed`, retained in request accounting; JRN-8 owns retry eligibility |

## Supported model scope

Provider work targets the current frontier series requested for this stage: GPT-5.6
(Sol/Terra/Luna), GPT-6 Astra, Claude Opus 5/Fable 5/Sonnet 5, and Gemini 3.8 Flash. Older models
are not a reason to retain compatibility branches. Configuration still names exact route/model
IDs, including proxy aliases; it does not guess backend capabilities from those names.

| Target | Request constraints | Official reference |
| --- | --- | --- |
| GPT-5.6 Sol/Terra/Luna | Explicit reasoning effort; no `minimal` | [Sol](https://developers.openai.com/api/docs/models/gpt-5.6-sol), [Terra](https://developers.openai.com/api/docs/models/gpt-5.6-terra), [Luna](https://developers.openai.com/api/docs/models/gpt-5.6-luna) |
| GPT-6 Astra | Use Responses for tools; `none` is not supported by the native model | [Model guide](https://developers.openai.com/api/docs/guides/latest-model) |
| Claude Opus 5/Fable 5/Sonnet 5 | Adaptive thinking and effort; no manual budget. Fable does not support disabled thinking | [Thinking](https://platform.claude.com/docs/en/build-with-claude/thinking) |
| Gemini 3.8 Flash | Native GenerateContent supports `low`/`medium`/`high`; no `minimal`, thinking budget or candidate-count setting | [3.8 guide](https://ai.google.dev/gemini-api/docs/generate-content/latest-model) |

The Chat adapter serves an explicitly selected compatible route. CLIProxyAPI's inspected
[server routes](https://github.com/router-for-me/CLIProxyAPI/blob/main/internal/api/server_routes.go)
expose the Gemini facade at `/v1beta/models/*action`, and its
[access provider](https://github.com/router-for-me/CLIProxyAPI/blob/main/docs/sdk-access.md)
accepts `X-Goog-Api-Key`. Select `google_generate_content` with a `/v1beta` proxy root; use the model
ID advertised by that instance. The proxy's
[Antigravity executor](https://github.com/router-for-me/CLIProxyAPI/blob/main/internal/runtime/executor/antigravity_executor.go)
owns OAuth and the CloudCode `/v1internal` upstream path. No Vertex/project wrapper is added to
Plexmaton's native request. This is source evidence, not verification of the installed proxy version
or its model access. Google's managed Antigravity agent uses Interactions and is a separate surface.

## Request configuration

`base_url` includes the API version path; selection never guesses a dialect from the URL.
[Provider examples](../../examples/providers.toml) show the route/model configuration.

| `api` | Path appended to the configured root | Authentication |
| --- | --- | --- |
| `openai_responses` | `responses` | Bearer key |
| `openai_chat_completions` | `chat/completions` | Bearer key |
| `anthropic_messages` | `messages` | `x-api-key`, `anthropic-version: 2023-06-01` |
| `google_generate_content` | `models/<id>:streamGenerateContent?alt=sse` | `x-goog-api-key` |

Model `instructions` defaults to empty and is bounded at 64 KiB. It encodes as Responses
`instructions`, a Chat system message, Messages `system`, or Gemini `systemInstruction`, and enters
the request-environment fingerprint. It comes from explicit user configuration, not project
instruction discovery or a second persisted transcript.

`reasoning_effort` defaults to `default`, which leaves the effort level to the provider. OpenAI
receives explicit effort directly. Messages uses disabled thinking for `none`; otherwise it requests
`thinking: {type: adaptive, display: summarized}`, including at default effort, and sets
`output_config.effort` only for an explicit level. This requests the
[visible summary](https://platform.claude.com/docs/en/build-with-claude/thinking#controlling-thinking-display)
without promising that every request produces one. Gemini always requests
[thought summaries](https://ai.google.dev/gemini-api/docs/generate-content/thinking#thought-summaries)
with `includeThoughts: true`. It accepts only `default`/`low`/`medium`/`high`, omitting `thinkingLevel`
for default effort; `none` is not silently mapped to low. The legacy `thinking_budget_tokens` field and `minimal` effort are rejected
before transport construction. `max_output_tokens` remains the output cap. Rejected: legacy manual
thinking paths, because they are outside the frontier target set. Per-model restrictions within a
dialect remain the endpoint's contract; unsupported options never trigger a silent downgrade.

`prompt_cache` defaults to `automatic`: OpenAI gets a 64-character hash of canonical session
identity as `prompt_cache_key`; Messages gets top-level `cache_control: {type: ephemeral}`; Gemini uses
implicit caching. `disabled` suppresses harness-added hints, not provider-internal caching. The key
is stable across turns, attempts and resume. Cache affinity follows each API's grammar and still
depends on the provider and exact prefix.

Messages [automatic caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching#automatic-caching)
targets the last cacheable block, including tools and history, without requiring `system`.
Rejected: block-only caching as the default, because the native API already advances its cache
point through the conversation; legacy Bedrock and gateway compatibility are outside this grammar.

## Native content and usage

| Dialect | Call/result grammar | Inclusive input | Generated output |
| --- | --- | --- | --- |
| Responses | `function_call` / `function_call_output`, joined by `call_id` | `input_tokens` | `output_tokens`, including reasoning |
| Chat | assistant `tool_calls` / role `tool`, joined by `tool_call_id` | `prompt_tokens` | `completion_tokens`, including reasoning |
| Messages | assistant `tool_use` / user `tool_result`, joined by `tool_use_id` | `input_tokens + cache_read_input_tokens + cache_creation_input_tokens` | `output_tokens`; optional `thinking_tokens` breakdown |
| GenerateContent | model `functionCall` / user `functionResponse`, optional upstream `id` | `promptTokenCount` | `candidatesTokenCount + thoughtsTokenCount` |

OpenAI totals are retained and checked. Messages has no total field, so its adapter derives the
sum once. Gemini validates a provided total or derives it from its components. Messages omitted
cache counters and Gemini omitted protobuf counters denote zero; OpenAI missing optional details
remain unknown. Gemini has no separately accounted cache-write category, so that category is zero.
Cumulative stream counters replace prior observations. A terminal persists the
latest observed report; cancellation cannot invent a later bill. BUD-2 owns measurement eligibility;
TIM-3 owns durable cost and aggregate coverage.

Messages `output_tokens_details.thinking_tokens` is an optional final-`message_delta` field in the
[thinking pricing reference](https://platform.claude.com/docs/en/build-with-claude/thinking-steering-and-cost#pricing).
Its absence leaves `reasoning_output` unknown and usage partial; it does not prevent pricing a
completed request. Nonzero hosted-tool usage is refused by both native adapters.

The native surface covers text, exposed thinking and local function tools. Multiple candidates,
unsupported content-bearing blocks and incomplete calls/signatures fail visibly. A cancelled or
failed unsigned thinking summary stays visible and durable but cannot become a signed wire block.
Messages omits an assistant message if filtering leaves no content; Responses omits the unsigned
reasoning item. Signed blocks remain unchanged, and unsigned reasoning in a tool batch is refused.
This follows Messages' [prior-turn thinking rules](https://platform.claude.com/docs/en/build-with-claude/thinking#preserving-thinking-blocks). Messages context
exhaustion maps to `ContextLimit` while retaining final usage; request rejection remains a model
error. `pause_turn` is explicitly unsupported because it requires a server-tool continuation;
`stop_reason` determines completion/refusal. Hosted tools, media, OAuth/subscription login, Vertex, Bedrock and Interactions are outside
these grammars.

Gemini's declared invalid-call, unexpected-call, tool-count, missing-signature and malformed-response
finishes become `ProviderFailed` with their wire code, retaining observed usage and dispatching no
calls. `TOO_MANY_TOOL_CALLS` is a tool-execution failure, not an output-token limit. Escalation and
account-policy stops map to `Refused`; image reasons remain unsupported. The
[v1beta discovery schema](https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta)
owns the exact enum. Stream error classification lives in the adapter. An in-stream rate limit
normally has no retry delay; it retains `Retry-After` only if the initial HTTP response supplied
that header. The synthetic 200-with-header fixture proves propagation, not provider behavior.

Generic native HTTP validation errors remain `ProviderFailed`; context length is inferred only from
a declared stable code, never provider message prose. Chat delta and Gemini part additions with
null, empty-array or empty-object values are ignored with field-name-only debug diagnostics.
Populated unimplemented fields are refused, including Chat annotations/legacy calls and Gemini
media metadata; their values never enter diagnostics. A non-tool finish with calls
refuses the batch, and inconsistent usage fails decoding under PRV-2/PRV-5. Rejected: treating these
as successful tool output or trustworthy accounting, because neither can be reconstructed safely.

Wire references: [Messages](https://platform.claude.com/docs/en/api/messages/create),
[Messages streaming](https://platform.claude.com/docs/en/build-with-claude/streaming),
[Messages stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons),
[GenerateContent](https://ai.google.dev/api/generate-content), and
[Gemini signatures](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures).

## Evidence

| Invariant | Proven by |
| --- | --- |
| PRV-1 | `prv_1_chat_fixture_drives_a_full_stateless_tool_round_trip`, `prv_3_responses_fixture_replays_encrypted_reasoning_exactly_and_round_trips_tools`, `prv_1_both_protocols_preserve_parallel_call_and_result_order`, `prv_1_chat_refuses_cross_kind_order_its_wire_cannot_represent`, `prv_1_protocol_selection_never_falls_back_across_replay_grammars`, `native_catalog_is_exact_unique_and_advertised_by_both_protocols`, `prv_1_messages_signed_tool_round_trip_normalizes_cumulative_usage`, `prv_1_gemini_tool_round_trip_preserves_signatures_and_counts_thoughts` |
| PRV-2 | Both fixture round trips, `prv_2_production_tool_argument_limit_remains_64_kibibytes`, `parallel_calls_share_one_aggregate_argument_bound`, `reverse_parallel_call_completion_is_sorted_before_dispatch`, `prv_2_both_protocols_bound_tool_identity_before_emission`, `assistant_output_rechecks_aggregate_text_and_tool_identity_bounds`, `oversized_semantic_text_fails_before_canonical_commit`, `maximal_valid_assistant_output_fits_the_journal_line_envelope`, `prv_2_rejects_duplicate_chat_tool_call_ids_before_emission`, `prv_2_rejects_incremental_duplicate_responses_tool_call_ids`, `prv_2_responses_counts_incrementally_completed_calls_toward_the_step_bound`, `prv_2_rejects_tool_arguments_before_they_can_reach_admission`, `prv_2_bounds_even_empty_responses_output_items`, `prv_2_responses_text_done_confirms_deltas_or_supplies_the_only_copy`, `prv_2_stopped_is_withheld_until_the_stream_trailer_is_valid`, `prv_2_sse_framing_rejects_partial_utf8`, `prv_2_messages_rejects_unfinished_mismatched_and_oversized_blocks`, `prv_2_gemini_call_ids_are_scoped_and_parallel_signatures_keep_their_part`, `prv_2_gemini_rejects_incomplete_duplicate_and_oversized_content` |
| PRV-3 | `prv_3_responses_fixture_replays_encrypted_reasoning_exactly_and_round_trips_tools`, `prv_3_replay_compatibility_covers_route_codec_revision_and_model_family`, `prv_3_replay_route_owner_encoding_is_unambiguous`, `prv_3_rejects_opaque_replay_before_step_state_can_grow`, `assistant_output_round_trips_order_and_redacts_replay`, `replay_only_wire_without_a_payload_is_rejected`, `reasoning_and_opaque_replay_survive_interrupt_without_sharing_presentation`, `provider_replay_is_named_and_bounded_before_turn_state_can_retain_it`, `prv_3_responses_phase_and_parts_survive_jsonl_reopen`, `prv_3_native_tool_conversations_survive_jsonl_reopen`, `prv_3_cancelled_calls_leave_no_dangling_replay`, `prv_3_chat_reasoning_aliases_round_trip_without_renaming`, `prv_3_gemini_signature_only_and_early_signature_parts_are_replayable`, `prv_3_interrupted_unsigned_reasoning_allows_durable_continuation`, `prv_3_unsigned_reasoning_in_tool_batches_is_still_refused` |
| PRV-4 | `prv_3_responses_fixture_replays_encrypted_reasoning_exactly_and_round_trips_tools` proves stateless full-record replay without a response ID, `prv_3_responses_phase_and_parts_survive_jsonl_reopen`, `prv_3_native_tool_conversations_survive_jsonl_reopen`, `prv_3_interrupted_unsigned_reasoning_allows_durable_continuation` |
| PRV-5 | `http_rate_limit_is_typed_and_keeps_retry_after`, `context_error_is_classified_by_wire_code`, `prv_5_responses_done_only_refusal_is_visible_and_typed`, and both fixture completion reasons, `prv_5_chat_rejects_structured_or_conflicting_reasoning`, `prv_5_messages_context_limits_and_refusals_keep_final_usage`, `native_stream_rate_limits_are_typed`, `gemini_declared_failures_preserve_diagnostics_without_dispatching_calls`, `messages_usage_refuses_unaccounted_server_tools`, `prv_5_gemini_policy_and_unsupported_image_finishes_are_distinct`, `stream_provider_errors_keep_their_category_and_observed_usage`, `dispatched_http_failures_preserve_their_terminal_measurements`, `provider_failure_reopens_as_the_same_non_retryable_outcome`, `prv_5_empty_additive_fields_preserve_text_and_populated_fields_fail`, `prv_5_messages_pause_turn_is_explicitly_unsupported` |
| PRV-6 | `prv_6_one_provider_resolves_two_exact_models_without_repeating_authority`, `prv_6_selection_and_every_model_fail_closed_before_network_work`, `prv_6_inline_authority_and_legacy_profiles_are_not_a_second_config_path`, `prv_6_resolution_rejects_unsafe_routes_without_echoing_them`, `prv_6_resolves_only_an_override_or_the_user_root`, `prv_6_key_resolution_is_explicit_and_redacted`, `cmd_2_selected_api_key_environment_is_removed_even_without_credential_shape`, `prv_6_standard_requests_omit_unspecified_reasoning_and_encode_instructions`, `prv_6_native_thinking_options_are_explicit_and_validated`, `prv_6_gemini_38_requests_use_only_frontier_thinking_controls`, `prv_6_messages_cache_and_thinking_match_native_request_contract`, `prv_6_provider_example_selects_each_documented_dialect`, `native_messages_http_uses_api_key_headers_and_terminal_usage`, `completed_messages_without_thinking_breakdown_keep_final_cost`, `native_gemini_http_uses_versioned_endpoint_and_header_authority` |
| PRV-7 | `event_guard_rejects_an_unterminated_event_at_the_bound`, `event_guard_rejects_one_oversized_transport_chunk`, `prv_2_and_prv_7_reject_unbounded_or_incomplete_provider_input`, `prv_7_model_config_cannot_widen_runtime_output_memory`, `interrupt_and_shutdown_cancel_and_join_the_exact_provider_task`, `native_stream_cancellation_preserves_observed_usage` |
