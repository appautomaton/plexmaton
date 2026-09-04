# Spec — Provider adapter

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Configuration, request encoding, streaming decode, reasoning replay, and typed failure at the model-provider boundary |
| Depends on | [agent-loop](./agent-loop.md) LOOP-1, LOOP-3 and LOOP-4 |
| Proven by | `plexmaton-provider::{config,codec,sse}` tests, sanitized protocol fixtures, `plexmaton-runtime::http` tests, and `plexmaton-agent::turn` tests |

## Invariants

**PRV-1 — Wire dialects end at one semantic port.** Responses and Chat Completions encode the same
ordered `ContextAtom`s and produce position-bearing semantic model events; neither owns a loop,
transcript, tool scheduler or approval path. API selection is explicit and never changes
mid-turn.

**PRV-2 — Streaming assembly is bounded and finality is explicit.** Text deltas pass through in
order; tool identity and argument fragments assemble under per-call, aggregate-step and count bounds,
and only a protocol completion event emits one whole call. Partial UTF-8, an unfinished call,
duplicate call identity, duplicate finality or a conflicting fragment is a typed malformed
response, never guessed content.
The production raw-call ceiling remains 64 KiB even when trusted canonical state reserves more;
a semantic stop is released only after the stream trailer passes validation.

**PRV-3 — Reasoning and replay are exact but separate.** Every exposed reasoning artifact is
retained in output order. Chat `reasoning_content` remains bounded semantic text; a Responses item
keeps its visible summary and block-anchored encrypted payload byte-for-byte. Opaque replay carries
adapter-owned route owner, codec revision and model family compatibility; mismatch is typed. It is
never rendered, copied, logged, truncated or treated as semantic text; exceeding its bound fails
the step visibly.

**PRV-4 — The semantic record is replay authority.** Every request is rebuilt from the record,
including exact provider replay sidecars and tool-call identities. A response ID is diagnostic
metadata only: the selected proxy reports `store: false`, and an unavailable `previous_response_id`
must not strand the session or create an adapter-private transcript.

**PRV-5 — Stops and failures stay typed.** Each protocol maps its declared completion, output-limit,
refusal, rate-limit, context, transport and malformed states without matching presentation text.
Unknown additive wire events are observable and ignored only when they carry no semantic content;
an unknown content-bearing event fails rather than silently losing output.

**PRV-6 — Configuration names data, never authority.** `~/.plexmaton/config.toml` separates named
provider routes from their named models and selects one exact provider/model pair. A route owns its
base URL, credential environment and default API; a model owns its wire/display identity, optional
API override, reasoning effort, context/output/reserve limits, estimator and optional price
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
ModelRequest + resolved model + tool schemas
                  │
        explicit protocol codec
          ┌───────┴────────┐
          │                │
      Responses      Chat Completions
          └───────┬────────┘
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
| Config asks for an unsupported reasoning effort | Typed configuration error before transport construction; no silent downgrade |

## Evidence

| Invariant | Proven by |
| --- | --- |
| PRV-1 | `prv_1_chat_fixture_drives_a_full_stateless_tool_round_trip`, `prv_3_responses_fixture_replays_encrypted_reasoning_exactly_and_round_trips_tools`, `prv_1_both_protocols_preserve_parallel_call_and_result_order`, `prv_1_chat_refuses_cross_kind_order_its_wire_cannot_represent`, `prv_1_protocol_selection_never_falls_back_across_replay_grammars`, `native_catalog_is_exact_unique_and_advertised_by_both_protocols` |
| PRV-2 | Both fixture round trips, `prv_2_production_tool_argument_limit_remains_64_kibibytes`, `parallel_calls_share_one_aggregate_argument_bound`, `reverse_parallel_call_completion_is_sorted_before_dispatch`, `prv_2_both_protocols_bound_tool_identity_before_emission`, `assistant_output_rechecks_aggregate_text_and_tool_identity_bounds`, `oversized_semantic_text_fails_before_canonical_commit`, `maximal_valid_assistant_output_fits_the_journal_line_envelope`, `prv_2_rejects_duplicate_chat_tool_call_ids_before_emission`, `prv_2_rejects_incremental_duplicate_responses_tool_call_ids`, `prv_2_responses_counts_incrementally_completed_calls_toward_the_step_bound`, `prv_2_rejects_tool_arguments_before_they_can_reach_admission`, `prv_2_bounds_even_empty_responses_output_items`, `prv_2_responses_text_done_confirms_deltas_or_supplies_the_only_copy`, `prv_2_stopped_is_withheld_until_the_stream_trailer_is_valid`, `prv_2_sse_framing_rejects_partial_utf8` |
| PRV-3 | `prv_3_responses_fixture_replays_encrypted_reasoning_exactly_and_round_trips_tools`, `prv_3_replay_compatibility_covers_route_codec_revision_and_model_family`, `prv_3_replay_route_owner_encoding_is_unambiguous`, `prv_3_rejects_opaque_replay_before_step_state_can_grow`, `assistant_output_round_trips_order_and_redacts_replay`, `reasoning_and_opaque_replay_survive_interrupt_without_sharing_presentation`, `provider_replay_is_named_and_bounded_before_turn_state_can_retain_it` |
| PRV-4 | `prv_3_responses_fixture_replays_encrypted_reasoning_exactly_and_round_trips_tools` proves stateless full-record replay without a response ID |
| PRV-5 | `http_rate_limit_is_typed_and_keeps_retry_after`, `context_error_is_classified_by_wire_code`, `prv_5_responses_done_only_refusal_is_visible_and_typed`, and both fixture completion reasons |
| PRV-6 | `prv_6_one_provider_resolves_two_exact_models_without_repeating_authority`, `prv_6_selection_and_every_model_fail_closed_before_network_work`, `prv_6_inline_authority_and_legacy_profiles_are_not_a_second_config_path`, `prv_6_resolves_only_an_override_or_the_user_root`, `prv_6_key_resolution_is_explicit_and_redacted`, `cmd_2_selected_api_key_environment_is_removed_even_without_credential_shape` |
| PRV-7 | `event_guard_rejects_an_unterminated_event_at_the_bound`, `event_guard_rejects_one_oversized_transport_chunk`, `prv_2_and_prv_7_reject_unbounded_or_incomplete_provider_input`, `prv_7_model_config_cannot_widen_runtime_output_memory`, `interrupt_and_shutdown_cancel_and_join_the_exact_provider_task` |
