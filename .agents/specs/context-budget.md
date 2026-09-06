# Spec — Context budget

| Field | Value |
| --- | --- |
| Status | Implemented; automatic compaction follows CPL-7 |
| Owns | Pure request-occupancy estimates, exact input-usage anchors and budget decisions |
| Depends on | JRN-5, TIM-3/TIM-4, PRV-3/PRV-6 |
| Proven by | Agent arithmetic/journal tests, provider codec estimates and runtime snapshot tests below |

## Invariants

**BUD-1 — The ledger is a projection.** One selected journal path and immutable request environment
produce one ledger without a write, model call, head mutation or stored parallel history. Its
snapshot contains counts and identities, never prompt text or opaque replay.

**BUD-2 — A measurement covers exactly its input.** Only reported agent-step input with the same
request environment, context epoch and exact ordered atom prefix on this path may anchor a ledger. The longest
matching prefix wins, with the latest authorization breaking ties; unavailable usage,
compaction requests and sibling paths cannot supply a measurement.
An individual request's missing optional breakdown does not invalidate its measured input;
aggregated partial counts never supply an anchor.
Rejected: using billed `total` or generated `output` as measured input occupancy, and subtracting
cache hits from occupied context.

**BUD-3 — Estimates retain their uncertainty.** Each whole atom and the request environment receive
a deterministic `utf8_heuristic_v1` estimate: serialized UTF-8 bytes divided by four, rounded up per
unit. Opaque replay bytes contribute only a flagged byte-size heuristic, never an exact token count
or an upper-bound promise; compatible encoding is validated before budgeting.

**BUD-4 — Limits and arithmetic are explicit.** Measured prefix plus estimated suffix (or the full
estimate without an anchor) is input occupancy; output reserve is separate. `Fits` includes the
soft boundary, aggregate pressure is `CompactionNeeded`, and an indivisible atom plus environment
that exceeds input capacity is `ImpossibleItem`. Arithmetic overflow and invalid limits are typed
errors; no wrapped count or missing measurement appears as zero.

## Model

The environment is included once: in the measurement when anchored, otherwise in the estimate.
The configured context window minus output reserve is input capacity; the default soft limit is
80% of that capacity. Maximum output remains a separate provider setting (PRV-6).
Per-atom estimates remain available for later compaction planning even under a measured prefix;
they do not partition the provider's measured count. A measured current input needs no estimated
suffix; a heuristic `Fits` is not a guarantee that a provider will accept the request.

The status-line snapshot projects this ledger alongside TIM-3 accounting. It does not parse
JSONL or calculate an independent budget. CPL-7 owns automatic compaction and dispatch gating.
`LiveRuntime::context_budget` exposes acknowledged facts on demand, not per frame. Pending writes,
failed persistence, incomplete tool batches and synthetic drivers without model configuration are
explicit unavailable states; other projection/encoding failures remain typed errors.

## Evidence

| Invariant | Proven by |
| --- | --- |
| BUD-1 | `bud_1_both_codecs_produce_redacted_deterministic_ledgers_without_writes`, `bud_1_runtime_snapshot_uses_the_configured_model_without_dispatch`, `bud_1_incomplete_tool_batch_cannot_produce_a_fit_snapshot`, `bud_2_anchor_uses_exact_input_and_survives_record_reload`, `cancelled_model_end_during_attempt_terminal_append_keeps_the_active_owner`, `failed_user_append_returns_the_draft_and_starts_no_effect`, `dropping_the_runtime_joins_its_journal_writer` |
| BUD-2 | `bud_2_anchor_uses_exact_input_and_survives_record_reload`, `bud_2_missing_and_changed_environment_have_no_anchor`, `bud_2_exact_input_with_missing_breakdowns_remains_an_anchor`, `bud_2_longest_prefix_wins_and_other_branches_are_excluded`, `bud_2_compaction_measurements_do_not_anchor_agent_context`, `bud_2_parallel_batch_anchors_require_every_result_in_model_order`, `bud_2_codec_environment_controls_measurement_reuse`, `bud_2_measured_prefix_replaces_estimates_without_double_counting_environment`, `checkpoint_preserves_history_and_refreshes_the_active_step`, `repeated_checkpoints_and_historical_forks_keep_their_own_epochs` |
| BUD-3 | `bud_3_unmeasured_and_opaque_inputs_keep_their_estimate_provenance`, `bud_3_opaque_replay_is_flagged_and_incompatibility_never_becomes_a_zero_estimate`, `bud_3_maximal_tool_results_are_estimated_as_one_indivisible_atom`, `bud_3_estimator_counts_utf8_wire_bytes_without_allocating_another_request_string`, `cpl_2_measured_overflow_refuses_compaction_without_rewriting_history` |
| BUD-4 | `bud_4_decisions_cover_soft_hard_reserve_and_indivisible_boundaries`, `bud_4_invalid_limits_anchors_and_overflow_are_typed`, `bud_2_measured_prefix_replaces_estimates_without_double_counting_environment` |
