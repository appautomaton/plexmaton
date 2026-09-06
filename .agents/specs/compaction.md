# Spec — Compaction

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Compaction planning, immutable checkpoint provenance, collected summarizer outcomes, bounded orchestration and the user's request for one |
| Depends on | BUD-1–BUD-4, JRN-1/JRN-3/JRN-5/JRN-7, PRV-1/PRV-3/PRV-4, TIM-2–TIM-5; [context epochs](../ui-ux.md#context-epochs-and-branch-selection) |
| Proven by | Agent, provider, JSONL and runtime tests below; the [spike](../spikes/compaction/README.md) retains source comparison |

## Invariants

**CPL-1 — Planning freezes one authoritative source.** A pure plan binds one head, revision,
semantic boundary, prior checkpoint epoch and request environment; its cut names stable atom
identities on that projection. No plan writes history, starts an effect, splits a JRN-5 tool batch,
or mutates opaque replay.

**CPL-2 — Summarization preserves the complete request prefix.** One stable instruction is appended
after the exact old history under the same model, instructions, tools and output configuration.
Each operation has one input and one attempt; overflow refuses compaction instead of deleting,
flattening, truncating or rewriting history, including opaque replay.
Rejected: fitted/lossy inputs, because even temporary history changes break the required cache prefix.

**CPL-3 — The replacement preserves the current request and exact retained context.** A checkpoint
projects its persisted summary, the latest user and its attached explicit skill when covered, and the exact retained
suffix, followed by later entries. Summary size, retained context and output reserve are budgeted
explicitly; no useful reduction, an oversized required user input or an unfittable environment is
a typed planning/publication refusal, never an unbounded compaction loop.

**CPL-4 — A checkpoint is an acknowledged journal fact.** One additive checkpoint entry records its
versioned plan and successful summarizer-attempt identity; that attempt's full output is already
durable in the same journal. Source revision, ancestry, cut, output, owner and environment are
validated before commit; JRN-7 acknowledgement precedes replacement publication or continuation.
Original entries remain intact; JRN-3 owns the current `plexmaton.session` schema epoch and foreign-epoch refusal.

**CPL-5 — Epochs belong to the selected ancestry.** Projection selects checkpoints only on the
target path, preserves the full visible history, and reconstructs the same context after reopen or
repeated compaction. Usage anchors require that path's checkpoint epoch and ordered projected
prefix, while historical forks preserve the original head and use their own nearest checkpoint.
Retention configuration selects future cuts; replay uses a checkpoint's persisted cut even after
that configuration changes. Rejected: reselecting an existing tail from current configuration,
original journal-position order for checkpoint context, and a source checkpoint as a historical-fork ban.

**CPL-6 — Summarization is its own model operation.** Every attempt has a real `CompactionId` owner
and distinct request-attempt identity, without a fabricated model step or durable user turn.
Bounded text, reasoning and replay output are retained with the terminal audit, including valid
partial output on failure; only complete, nonempty, bounded text without tool calls can publish a
checkpoint. Summary tool calls never reach admission or execution; TIM-3 accounts each attempt once.

**CPL-7 — Automatic work has one bounded owner.** Pre-turn soft pressure, complete post-tool hard
pressure and one empty-output typed context-error recovery are routed through the owned runtime.
At most three separate compaction operations and one agent context-error recovery occur per turn;
there is no changed-input summary retry. Cancellation, timeout, accepted writes and shutdown retain
observable completion and no detached work. If the complete history plus instruction cannot fit,
hard pressure stops the step before any summary request.

**CPL-8 — Failure preserves a usable authoritative state.** Definite planning or summary failure
does not publish a checkpoint or alter the source head; a soft failure may continue the old request
only while it remains within the hard budget. Hard failure is visible, and uncertain persistence
keeps JRN-7's freeze/reopen rule; neither retry nor model/tool dispatch crosses an unacknowledged fact.

**CPL-9 — A request is one idle attempt with nothing to continue.** `/compact` asks the runtime
for one compaction of the selected head. It is admitted only while idle: no turn, no pending
approval, no owned compaction, shutdown not begun. Every other state, a missing budget, and a plan
that finds nothing to replace or nothing that fits, is a typed refusal that writes no record. An
admitted request follows CPL-1–CPL-6 and CPL-8 unchanged: one authorization, one attempt, one
checkpoint, no fabricated step and no model call afterwards. Text submitted while it runs waits
in the runtime's bounded input queue and opens its turn once the request ends, so the first
request after `/compact` already starts from the summary; interrupt and shutdown cancel and join
the request and return the waiting text as they do queued input. The outcome, published or failed
with its kind, reaches the composition root as a report beside the attempt's visible failure.
Rejected: returning typed text to the composer while the summarizer runs, which made the user
send again what they had already said; and retrying a failed request, because the user can ask
again.

## Model

The agent crate owns semantic types, checkpoint validation/projection and the staging entrypoints.
The provider crate owns codec-based estimates and prepares one append-only request.
The runtime owns scheduling, transport, collection and commit ordering. The ordinary
agent loop still receives only its own steps; it resumes the same pending step with freshly
projected context after a checkpoint. A requested compaction takes the same path from a pure plan
to an acknowledged checkpoint and then reports instead of refreshing a step.

```text
acknowledged context -> pure plan -> Compaction attempt authorization -> ack
    -> collected model operation -> Compaction attempt finished (audit + full output) -> ack
    -> checkpoint entry referencing that output -> ack -> refreshed agent request
```

The intended narrow data surfaces are:

- `ContextEpoch`: original context or a checkpoint `ConversationEntryId`; derived from ancestry.
- `CompactionSource`: selected `HeadName`, `HeadRevision`, semantic boundary and `ContextEpoch`.
- `CompactionCut`: first/last covered atom identity, optional first retained atom identity, and an
  optional covered latest-user identity to preserve. An atom's first source entry identifies it;
  validation resolves the complete atom rather than cutting among its source entries.
- `CompactionPlan`: operation ID, source, cut, `RequestEnvironment` and maximum summary text bytes.
  It stores only bounded descriptors; the summarizer request remains a transient projection.
- `CompactionInputMode`: the durable `verbatim` audit marker; there are no rewriting modes.
- `CompactionAttemptFinished`: ordinary terminal audit, input mode, and either complete
  `AssistantOutput` or a typed failure with optional valid partial output. This is one new
  non-advancing `JournalRecord`; existing record variants gain no required fields.
- `CompactionCheckpoint`: versioned plan plus successful `RequestAttemptId`. Its context summary
  is a distinct semantic atom, encoded as harness-supplied user context by each existing codec;
  summarizer reasoning/replay stays in the attempt record and outside the visible transcript.

Checkpoint coverage uses stable range endpoints against its frozen source projection. It does not
duplicate all covered entries, original text or tool outcomes into a second history. Source
selection and complete-atom validation make missing, reordered or foreign provenance a typed error.

The default planning policy reserves up to one quarter of available input for summary text,
subject to the model's existing reserve and a 64 KiB summary-text cap. Each model's
`compaction_keep_recent_tokens` defaults to `20000`; its effective suffix target is the smaller
of that value and one quarter of input capacity after environment occupancy. Selection retains
whole newest atoms within that target and preserves the latest user exactly. A covered latest user
is pinned after the digest. The target is approximate: atoms are never split and required user
content is not truncated. Its immediately attached SKL-5 activation is part of that required context;
both preview and journal projection retain and budget the same pair. Older activations may be summarized.
This policy controls checkpoint projection, never the summarizer input.
Zero is rejected. The setting does not enter wire encoding or the environment fingerprint;
changing it leaves existing checkpoint context unchanged, including after reopen and subsequent
user turns. Only a new compaction selects a new cut under the new target and starts a new epoch.
Impossible required context fails explicitly. Actual request encoding is estimated by
the same BUD-3 implementation as ordinary context. A heuristic fit is not a provider guarantee;
publication also checks the actual resulting context and useful reduction. Reduction is measured
with codec token estimates, not atom count: replacing a large tool batch with a summary can keep
the same number of atoms. The journal validates structural coverage independently of that estimate.

Failed summarization can retain reasoning/replay for audit without making it a continuation token.
Only the accepted summary text and original retained context enter the replacement; the provider's
session cache-affinity hint is independent of the semantic checkpoint identity.

The appended [prompt](../../crates/plexmaton-provider/src/compaction/prompt.md) requests a concise
state handoff: objective/constraints, current state, open work and essential references. It carries
forward applicable earlier summaries, distinguishes observed results from assumptions, and keeps
exact identifiers needed for continuation. Essential facts stay in the digest even when recent:
the prompt cannot assume which facts the bounded tail retains. It records the exact continuation
point, applies the latest user corrections and keeps next steps within unfinished requested work,
without reviving completed or abandoned tasks. Tools stay advertised,
while the prompt requests no tool use and CPL-6 rejects any returned calls. Prompt wording is reviewed
offline; cache hits and model-generated summary quality are not live-test requirements.

## Evidence

| Invariant | Proven by |
| --- | --- |
| CPL-1 | `cpl_2_compaction_appends_only_the_instruction_across_all_dialects`, `checkpoint_publication_rechecks_all_frozen_provenance`, `stale_checkpoint_publication_mutates_nothing` |
| CPL-2 | `cpl_2_compaction_appends_only_the_instruction_across_all_dialects`, `non_context_summary_failure_does_not_retry`, `cpl_2_measured_overflow_refuses_compaction_without_rewriting_history` |
| CPL-3 | `cpl_3_skill_invocation_survives_compaction_in_every_dialect`, `cpl_3_replacement_preview_rejects_oversized_summary_without_mutating_source`, `equal_atom_count_checkpoints_replace_huge_batches_and_prior_summaries`, `checkpoint_publication_rechecks_all_frozen_provenance`, `cpl_3_planning_refusals_are_typed_and_leave_source_unchanged`, `cpl_3_replacement_preview_rejects_token_non_progress`, `cpl_3_configured_recent_tail_changes_only_the_checkpoint_cut`, `cpl_3_recent_tail_target_is_capped_by_available_input` |
| CPL-4 | `compaction_attempt_and_checkpoint_each_wait_for_ack_before_continuation`, `schema_2026_09_04_fixture_reopens_projects_and_continues`, `checkpoint_fixture_reopens_and_historical_fork_keeps_original_epoch`, `collected_attempt_record_round_trips_without_debugging_replay` |
| CPL-5 | `checkpoint_preserves_history_and_refreshes_the_active_step`, `repeated_checkpoints_and_historical_forks_keep_their_own_epochs`, `cpl_5_repeated_checkpoints_and_historical_forks_reopen_with_identical_wire_bytes`, `cpl_5_retention_config_change_preserves_checkpoint_and_continuation_bytes` |
| CPL-6 | `cpl_6_summary_http_preserves_environment_output_and_accounting_across_dialects`, `cpl_6_summary_http_rejects_tools_and_keeps_their_output_for_audit`, `cpl_6_summary_http_failures_keep_raw_terminal_and_partial_output`, `cpl_6_collector_keeps_cancelled_partial_output_and_bounds_block_growth`, `collected_attempt_validation_distinguishes_success_from_partial_failure` |
| CPL-7 | `skill_preparation_completes_while_compaction_is_waiting`, `soft_pre_turn_compaction_uses_a_distinct_owner_and_refreshes_after_checkpoint`, `post_tool_hard_pressure_preserves_history_and_dispatches_nothing`, `typed_context_error_recovers_the_same_step_once`, `context_error_after_output_does_not_start_compaction`, `summary_context_pressure_does_not_retry_with_changed_input`, `compaction_timeout_cancels_and_joins_before_continuation`, `interrupt_cancels_and_joins_the_owned_compaction`, `shutdown_cancels_and_joins_the_owned_compaction`, `interrupt_during_compaction_authorization_never_dispatches_the_summarizer`, `shutdown_during_compaction_authorization_never_dispatches_the_summarizer`, `interrupt_during_compaction_terminal_ack_starts_no_continuation`, `shutdown_during_checkpoint_ack_starts_no_agent_continuation`, `interrupt_during_refreshed_agent_authorization_starts_no_provider`, `cpl_7_turn_compaction_limits_are_bounded_independent_and_reset` |
| CPL-8 | `failed_attempt_keeps_the_frozen_source_usable`, `failed_compaction_diagnostic_reopens_without_exposing_partial_output`, `post_tool_hard_pressure_preserves_history_and_dispatches_nothing`, `uncertain_checkpoint_append_freezes_before_agent_continuation`, `cancelled_compaction_terminal_append_keeps_the_operation_owned` |
| CPL-9 | `cpl_9_requested_compaction_publishes_a_checkpoint_and_dispatches_no_step`, `cpl_9_a_running_step_refuses_the_request`, `cpl_9_an_owned_compaction_and_shutdown_refuse_the_request`, `cpl_9_a_waiting_approval_refuses_the_request`, `cpl_9_planning_refusals_are_typed_and_write_nothing`, `cpl_9_interrupt_cancels_a_requested_compaction_and_reports_it`, `cpl_9_shutdown_cancels_a_requested_compaction_and_dispatches_nothing`, `cpl_9_text_during_a_requested_compaction_waits_for_the_checkpoint`, `cpl_9_failed_and_timed_out_requests_report_their_kind_and_keep_the_head` |
