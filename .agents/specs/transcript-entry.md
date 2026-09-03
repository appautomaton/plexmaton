# Spec — Transcript entry

| Field | Value |
| --- | --- |
| Status | Identity, lifecycle, retained presentation, compact projection, disclosure and semantic copy implemented through Phase 01 stage 3 slice 5; diff styling remains slice 6 |
| Owns | Stable transcript identity, first-appearance order, entry revisions, tool lifecycle updates, typed presentation, and replay reduction |
| Depends on | [transcript-layout](./transcript-layout.md) TR-1 and [tool-admission](./tool-admission.md) APV-5/APV-6 |
| Proven by | `plexmaton-core::transcript`, `plexmaton-agent::{record,tools,turn}`, `plexmaton-file-tools`, `plexmaton-command`, `plexmaton-runtime`, and `plexmaton-tui::{content,transcript,state}` tests |

## Invariants

**ENT-1 — First appearance fixes identity and order.** The producer assigns every transcript fact a
`TranscriptItemId`; text, tools, artifacts, mail, warnings and errors share one per-agent ordered
projection. Mail is owned by its producer while retaining both delivery endpoints. A tool's
`ToolCallId` and other domain IDs correlate facts but never choose their position, and a vector
index is not an identity.

**ENT-2 — One tool call is one revisioned entry.** A call first appears queued at revision zero and
each accepted lifecycle transition advances exactly one revision on the original entry. Display
order is first appearance, execution completion order is event order, and model result order stays
the batch's model-call order (APV-5). Rejected: moving a completed call to the tail or using its
completion position as model order.

**ENT-3 — Replay is pure reduction.** Feeding the same ordered envelopes to a fresh projection
produces the same state and can only mutate that projection; it cannot contact a provider, consult
policy, request approval, or execute a tool. Sequence gaps and invalid identities, revisions,
correlations or transitions are retained in the bounded notice log rather than becoming transcript
facts. A restored pending approval remains subject to APV-6.

**ENT-4 — Open and copy project retained semantic detail.** Tool presentation distinguishes text
from a canonical diff, distinguishes omitted bytes from empty text, and keeps invocation separate
from outcome. Admission contributes a bounded canonical invocation; every later lifecycle update
retains it, and execution or a no-run terminal contributes a bounded outcome. Plain text retains
at most 64 KiB with explicit omitted-byte metadata. A successful exact edit retains its complete
canonical patch under the bound derived by MUT-6; unchanged file bytes never enter it. Slice 2
proves production and retention bounds. Disclosure is view state keyed by `TranscriptItemId`, never
another session fact: it survives lifecycle replacement, changes one cached height, and expands
inside the parent conversation rather than creating a nested viewport. `Ctrl-O` addresses the
selection's moving end; a completed pointer gesture selects and addresses the same entry, while
hover is visual only. Copy returns the retained invocation then outcome without the disclosure's
headings, gutters, clipping, styling, or omission label. Diff source already follows this path;
its distinct added/removed treatment remains unproven until Phase 01 stage 3 slice 6.

## Model

```text
producer counter ─▶ TranscriptItemId ─▶ first appearance ───────────┐
                                                                  ▼
tool lifecycle ─▶ same id + next revision ─▶ pure ViewState reducer
                                                                  │
domain identity ───────────────────────────── correlation only ────┘
```

Text deltas and finalization use the same revision rule as tool transitions. Terminal entries such
as mail and artifacts stay at revision zero because they have no update vocabulary yet.

## Failure modes

| Situation | Response |
| --- | --- |
| An entry identity appears twice | Reject the later event and retain a notice |
| An entry identity is presented under another agent | Reject the event; its original owner remains authoritative |
| An update skips or repeats a revision | Reject it without mutating the entry |
| A tool entry changes call identity or label | Reject the correlation change |
| A tool skips its lifecycle or leaves a terminal state | Reject the transition |
| Sibling tools complete out of order | Update both original positions; preserve batch result order separately |
| The same envelopes are replayed into a fresh projection | Reconstruct equal state without effects |

## Evidence

| Invariant | Proven by |
| --- | --- |
| ENT-1 | `every_transcript_identity_is_new_and_names_its_agent`, `every_transcript_category_enters_one_ordered_projection`, `mail_retains_both_endpoints_and_lives_with_its_producer`, `an_entry_identity_cannot_move_between_agents`, `shuffled_tool_completions_update_their_original_entries`, `interleaved_text_and_tools_keep_their_positions_when_tools_finish_out_of_order` |
| ENT-2 | `a_step_that_asked_for_tools_dispatches_them_and_waits`, `production_tool_lifecycles_replay_as_one_entry_each`, `tool_lifecycle_allows_only_forward_declared_transitions`, `tool_updates_refuse_revision_gaps_and_invalid_transitions`, `results_are_assembled_in_the_order_the_model_asked_and_not_the_order_they_finished`, `every_tool_status_is_one_named_logical_line`, `a_tool_transition_remeasures_only_its_original_entry` |
| ENT-3 | `every_event_variant_survives_a_json_round_trip`, `two_fresh_projections_of_the_same_envelopes_are_equal`, `rejected_event_does_not_block_the_rest_of_the_stream`, crate-graph gate |
| ENT-4 | Presentation production/bounds: `bounded_presentation_text_carries_exact_omission_metadata`, `tool_status_updates_accumulate_invocation_and_outcome_presentation`, `cancellation_before_admission_has_outcome_without_invocation`, `maximum_valid_edit_retains_a_complete_bounded_patch`, `file_observation_survives_the_runtime_boundary_into_an_approved_edit`, `maximal_command_result_stays_bounded_in_the_next_model_request`; disclosure/copy: `every_tool_status_can_disclose_the_same_typed_detail`, `ctrl_o_opens_the_selections_focus_entry_in_place_at_each_drawn_width`, `pointer_and_ctrl_o_toggle_the_same_item_while_drag_cancels_disclosure`, `tool_completion_preserves_the_users_open_state`, `tool_copy_preserves_every_retained_source_in_producer_order`, `tool_copy_is_identical_when_compact_open_resized_scrolled_and_monochrome`, `the_open_tool_frames_match_their_fixtures`; distinct diff styling remains unproven until Phase 01 stage 3 slice 6 |
