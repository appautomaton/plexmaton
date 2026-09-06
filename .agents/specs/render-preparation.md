# Spec — Owned render preparation

| Field | Value |
| --- | --- |
| Status | Text and native math preparation connected to the live TUI |
| Owns | Shared preparation payload, process lifetime and completion admission |
| Depends on | FR-2/FR-3, MD-4/MD-5, MTH-4 |
| Proven by | TUI admission/copy tests, codec and real-process tests, the production-loop blocking witness and release measurements |

## Invariants

**PRE-1 — Preparation carries source identity, not terminal authority.** A bounded request and
reply preserve agent, entry, source revision, width, disclosure and math capability along with palette-neutral
rows, atomic formula rectangles and copy ranges. The private child dispatch occurs before configuration and terminal setup;
it reads only piped requests and returns framed data, never terminal commands (MD-4/MD-5). A streaming
Markdown request may carry a bounded, parser-signature-checked prefix layout; the complete source
crosses the boundary, where the worker validates its full-parser event pass and renders only suffix
events. Invalid hints take canonical preparation; optional hint bytes are discarded before refusing
a source that fits the request budget alone. Text fragments validate their checked column plus exact
grapheme width against the requested width; atomic formula geometry remains separately validated.

**PRE-2 — Cancellation ends computation before replacement.** One owner retains at most one
active operation and one latest pending batch, keeps partial I/O across select interruptions,
and kills and reaps its child on replacement, timeout or shutdown. Cleanup failure quarantines
replacement; idle owns no periodic wake (MTH-4/FR-1). Rejected: aborting a blocking future, which
discards a result without terminating the synchronous parser.

**PRE-3 — Completion cannot move unseen text beneath input.** Adoption matches an owned request
in the current workspace generation and its exact source/geometry identity; a successful frame
alone replaces the immutable pinned text hit map (FR-3). Missing or failed preparation is local
presentation, never permission for draw, input or selection validation to invoke a parser.
MD-4's retained presentation carries its own source identity through height measurement, paint,
native reservations and pointer mapping, even when a completion is overtaken by another delta.
MTH-5 requires both cell and native output success before that publication.

**PRE-4 — Selected-text copy waits for data, not on the input loop.** Release captures painted
fragments and retains a bounded assembly with every member's observed revision/disclosure for
cancellation and missing-entry preparation. The same owner fills unpainted gaps and emits one
complete copy (SEL-1/SEL-2/SEL-4). Captures iterate only the bounded painted set and account for
container capacity plus optional fragment allocation; an empty member is a captured absence.
A new explicit Copy captures the current painted representation. Changed members, a new
selection or another copy cancel obsolete delivery; failure and capacity limits are explicit.

## Evidence

| Invariant | Proof |
| --- | --- |
| PRE-1 | `real_preparation_worker_preserves_the_complete_native_math_reply`, `real_preparation_worker_reuses_streamed_markdown_prefixes`, `preparation_wire_drops_optional_prefixes_before_refusing_source`, `frozen_prefix_rejects_malformed_hint_copy_ranges`, `preparation_wire_rejects_a_text_fragment_at_the_right_edge`, `preparation_wire_rejects_mismatched_math_capability_geometry_and_atomic_maps`, `oversized_preparation_identity_never_enters_the_cache_or_request_queue`, `oversized_literal_pending_frame_is_bounded_before_worker_admission`, `real_preparation_driver_round_trips_semantic_rows_at_three_widths`, `preparation_driver_eof_exits_without_configuration_or_terminal_output`, `preparation_wire_rejects_mismatched_identity_and_invalid_copy_ranges`, `preparation_wire_bounds_requests_before_retaining_them`, `preparation_driver_rejects_truncated_frames_and_unadmitted_modifiers`, `malformed_preparation_replies_cannot_attach_or_escape_the_output_bound`; checking only width fails the cross-agent identity witness |
| PRE-2 | `preparation_replacement_reaps_before_starting_only_the_latest_batch`, `preparation_retains_partial_pipe_io_across_select_interruptions`, `preparation_shutdown_cancels_active_and_pending_without_an_idle_wake`, `preparation_timeout_kills_and_reaps_computation`, `blocked_preparation_never_holds_the_production_input_and_frame_loop`, `cleanup_failure_on_an_old_ticket_settles_the_latest_workspace_request`; omitting reap and awaiting preparation inline each fail their witness. Actual OS cleanup-timeout injection remains unproven |
| PRE-3 | `a_prepared_revision_overtaken_by_queued_deltas_is_still_painted_and_copyable`, `streaming_preparation_keeps_the_last_painted_rows_and_geometry`, `failed_native_output_keeps_the_last_painted_hit_map_and_frame_identity`, `replacing_projection_revokes_math_work_even_when_semantic_keys_are_identical`, `cold_preparation_is_deferred_and_hidden_rich_history_is_not_queued`, `preparation_cannot_cross_workspace_generations_or_admit_mismatched_keys`, `prepared_text_is_not_selectable_until_the_result_has_been_painted`, `superseded_preparation_is_ignored_and_failure_is_local_without_an_idle_retry`, `real_preparation_process_drives_painted_rows_and_exact_pointer_copy`, `late_real_reply_cannot_attach_to_a_replaced_workspace`, `live_preparation_splits_capacity_batches_without_losing_valid_entries`; comparing only sequence fails the workspace-generation witness |
| PRE-4 | `large_selection_capture_is_bounded_by_the_painted_set`, `empty_painted_fragments_do_not_copy_unseen_text_or_wait_for_it`, `pending_stream_copy_captures_painted_fragments_without_waiting_for_new_source`, `ready_preparation_keeps_pointer_copy_on_the_painted_source_until_the_next_frame`, `selected_text_capacity_refuses_a_complete_request_without_emitting_a_prefix`, `selected_text_waits_for_missing_preparation_and_emits_one_complete_copy`, `pending_copy_cannot_outlive_selected_source_changes_or_cancellation`, `prepared_text_is_not_selectable_until_the_result_has_been_painted` |

Reviewed pending frames at [120](../../crates/plexmaton-tui/frames/preparation/pending-120.svg),
[88](../../crates/plexmaton-tui/frames/preparation/pending-88.svg) and
[60](../../crates/plexmaton-tui/frames/preparation/pending-60.svg), with matching `unavailable-*`
and `copy-pending-*` frames alongside them. All nine use the actual workspace projection.

## Model and limits

The TUI owns pure preparation and retained presentation data. The CLI library's concrete owner is
shared by the executable and real-process measurement harness. It owns framed pipes and a
single persistent child of the current executable, with an empty environment, no terminal
handles and discarded stderr. The child performs no configuration, provider or storage startup.
Its synchronous engine is isolated so cancellation can actually end CPU work. The main process
keeps terminal output ownership. This is a private same-build protocol, not a compatibility API.
The live adapter polls that owner beside input, clipboard, runtime and frame deadlines; all exits
join it. Workspace generations are retained identity tokens, without a global counter. Frames
declare reached keys, not queued source clones. Capacity-refused batches halve to one entry before
an individual refusal becomes visible; a successful batch restores the sixteen-entry ceiling.

| Boundary | Limit |
| --- | --- |
| Active / pending | One retained operation and one latest encoded batch; no per-request detached task |
| Request | 16 entries; 192 KiB of snapshots and prefix hints admitted before cloning, 256 KiB encoded; a Markdown prefix hint is capped at 64 KiB and length prefix is checked before allocation |
| Reply | 2 MiB encoded bytes and aggregate prepared allocation; oversized batches return a typed refusal |
| Prepared entry | 1 MiB allocation, 8,193 rows; 4 KiB identity admission; validated UTF-8 copy ranges, checked grapheme-width text fragments, atomic rectangle/run consistency and selection-padding bounds |
| Frame pins | 128 entries / 4 MiB per candidate and last-painted map, separately bounded from MD-4's LRU |
| Selected-text assembly | One selection, 8 MiB including retained member identities and text capacity; no truncation or delivery acknowledgement |
| Process | Absolute executable, empty environment, piped stdin/stdout, discarded stderr |
| Lifetime | 2 s covering request/reply I/O and computation; another 500 ms for kill/reap; uncertain cleanup retains the child in quarantine |

Pending revisions preserve compatible prepared content and its height; cold or evicted entries
use a placeholder with a known or estimated height. Unavailable entries show a compact refusal
without guessed text ranges. Source
copy remains available, including when an individual entry exceeds preparation limits. Plain
hidden text counts borrowed row breaks; unknown rich heights remain explicit estimates until
reached. Ordinary CLI gates use the real process. CPU-only fixtures and reference measurements
explicitly pump the shared pure batch operation outside `Workspace::draw`.

FR-4 owns the measured CPU, process and live-adoption costs, including the paired batch experiment.
Terminal transport and outer input wait are excluded; those figures are not end-to-end latency
guarantees. Byte limits do not establish an OS memory guarantee.
