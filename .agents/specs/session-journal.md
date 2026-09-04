# Spec — Session journal

| Field | Value |
| --- | --- |
| Status | Structural reducer, pure projections and per-session JSONL adapter implemented |
| Owns | Stable records, immutable entry ancestry, named-head revisions, lossless wire form, exclusive file writer and final-tail recovery |
| Depends on | PRV-3/PRV-4 for model replay, ENT-1/ENT-3 for transcript identity and pure reduction |
| Proven by | `plexmaton-agent::journal` and `plexmaton-session-store` tests |

## Invariants

**JRN-1 — One record is one complete mutation.** An entry append carries its parent, target head and
expected head revision together; applying it advances that head or changes nothing. Head creation
requires a fresh never-used name; movement, rename and abandonment are likewise revision-checked
records. Sequence, record and entry identities never repeat.

**JRN-2 — Reduction is deterministic and typed.** The same ordered records build equal entries,
heads and paths. A sequence gap, unknown parent/head, stale revision or reused/retired identity is a
typed refusal which mutates nothing.

**JRN-3 — The wire is lossless without weakening opaque replay.** Every record has one tagged JSON
form and decodes through the constructors that protect identities and `ProviderReplay`. Lossless
serialization includes the exact replay payload; `Debug` and decoding errors reveal no payload.

**JRN-4 — One record is one write, and loading keeps a valid prefix.** The JSONL adapter appends
each record in one unbuffered `write` and never `fsync`s, so
bytes a returned append handed the kernel outlive the process that dies, and a power loss costs the
tail the page cache had not flushed. Loading repairs a complete final value missing its newline,
isolates an incomplete tail, and refuses to guess past earlier corruption. Journal, fork staging
and isolated-tail files are owner-only; the sessions directory becomes owner-only when Slice 6
creates it. Rejected: per-record
`fsync`; stronger power-loss ordering on macOS would require `F_FULLFSYNC`, and neither buys a
process-death guarantee that unbuffered writes do not already provide. Also rejected: a database or
on-disk index before a measured query need, when an append-only log has no in-place mutation for one
to make safe.

**JRN-5 — Replay performs no effects.** Walking one selected head derives a complete `ModelRequest`
and a freshly numbered `SessionEventEnvelope` stream without invoking a provider, tool, approval
policy or filesystem operation. Tool results enter the request in model-call order even when their
visible terminal transitions arrived out of order. An incomplete final batch stays visible, is
omitted as a whole from provider input, and returns a typed recovery projection; the same condition
before a later model fact is corruption rather than a silently ignored tail. Completed messages
normalize provider chunking into one replay delta; chunk boundaries are transport facts, not durable
session semantics.

## Model

```text
JournalRecord ──▶ SessionJournal
                     ├─ entries: EntryId → { parent, payload }
                     ├─ heads: HeadName → { target, revision }
                     └─ ordered records

AppendEntry { head, expected_revision, entry.parent_id }
CreateHead | MoveHead | RenameHead | AbandonHead
```

`main` exists at revision zero and points to no entry in a new session. Abandoned and renamed-away
head names remain retired so a stale command cannot become valid after a name is reused.

## Failure modes

| Situation | Response |
| --- | --- |
| Record sequence is not exactly next | Typed unexpected-sequence refusal; no state changes |
| Append parent differs from the named head | Typed parent mismatch; no implicit branch |
| Head revision changed after a record was prepared | Typed stale-revision refusal |
| Parent or head target is unknown | Typed missing-identity refusal |
| Record, entry or retired head name is reused | Typed duplicate/retired refusal |
| Opaque replay exceeds its byte bound during decode | Decode fails before a journal can retain it |

## Evidence

| Invariant | Proven by |
| --- | --- |
| JRN-1 | `jrn_1_append_and_head_mutations_form_one_checked_tree` |
| JRN-2 | `jrn_2_invalid_records_change_nothing`, `jrn_2_the_same_records_build_equal_journals_and_paths`, `jrn_2_each_head_mutation_rejects_a_stale_revision`, `jrn_2_each_head_mutation_rejects_a_missing_head`, `jrn_2_an_unknown_append_parent_is_a_missing_entry`, `jrn_2_head_names_are_never_reused` |
| JRN-3 | `jrn_3_every_record_round_trips_and_debug_redacts_replay`, `jrn_3_every_model_item_variant_round_trips_inside_an_append`, `jrn_3_every_canonical_payload_variant_round_trips_inside_an_append`, `jrn_3_decoding_rechecks_identity_and_replay_bounds`, `jrn_3_and_jrn_4_encrypted_replay_round_trips_through_the_file` |
| JRN-4 | `jrn_4_create_append_reopen_and_immediate_visibility`, `jrn_4_a_second_writer_is_refused_until_the_owner_closes`, `jrn_4_valid_final_record_without_newline_is_repaired`, `jrn_4_incomplete_final_tail_is_isolated`, `jrn_4_middle_corruption_is_not_guessed_around`, `jrn_4_unknown_format_version_is_refused`, `jrn_4_invalid_sequence_is_refused_even_on_the_final_line`, `jrn_4_unknown_final_record_kind_is_a_schema_failure`, `jrn_4_duplicate_record_field_is_refused_without_tail_recovery`, `jrn_4_terminated_invalid_final_line_is_not_tail_recovery`, `jrn_4_write_failure_changes_no_memory_and_requires_reopen`, `jrn_4_partial_write_reopens_at_the_last_complete_record`, `jrn_4_newline_write_failure_recovers_the_record_as_committed`, `jrn_4_oversized_record_is_returned_without_poisoning_the_writer`, `jrn_4_failed_header_encoding_leaves_no_file`, `jrn_4_rejected_append_writes_nothing_and_returns_exact_ownership`, `jrn_4_unterminated_line_cannot_grow_past_the_bound_when_repaired`, `jrn_4_poisoned_writer_cannot_fork`, `jrn_4_fork_publishes_a_complete_sibling`, `jrn_3_and_jrn_4_journal_fork_and_tail_files_are_owner_only`, `jrn_3_and_jrn_4_insecure_existing_journal_is_refused`, `jrn_3_and_jrn_4_encrypted_replay_round_trips_through_the_file` |
| JRN-5 | `jrn_5_canonical_live_turn_and_journal_replay_have_equal_model_context`, `jrn_5_multi_delta_live_turn_and_replay_have_equal_visible_semantics`, `jrn_5_one_path_projects_model_order_and_visible_lifecycle`, `jrn_5_incomplete_tool_batch_is_explicit_and_absent_from_the_request`, `jrn_5_incomplete_tool_batch_before_later_content_is_rejected`, `jrn_5_system_message_after_incomplete_batch_remains_a_recoverable_tail`, `jrn_5_named_heads_project_only_their_selected_ancestry`, `jrn_5_hidden_replay_and_visible_diagnostics_project_to_their_exact_consumers`, `jrn_5_invalid_tool_lifecycle_has_a_typed_projection_error`, `jrn_5_duplicate_transcript_identity_is_rejected_before_projection`, `jrn_5_mail_requires_both_visible_endpoints`, `jrn_5_attention_resolution_keeps_its_request_owner`, `jrn_5_tool_presentation_accumulates_across_lifecycle_snapshots`, `jrn_5_journal_projection_builds_the_model_request_and_tui_state`, crate-graph gate |
