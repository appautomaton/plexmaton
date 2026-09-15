# Spec — Collaboration ledger

| Field | Value |
| --- | --- |
| Status | Implemented and wired. Accepted: `scripts/smoke-delegate.py` proves durable delegation/mail and owned child Stop; COL-3 Handoff and post-transfer User input have tier-2 backend evidence, with product input projection owned by stage 7.6 |
| Owns | Canonical collaboration admission, retry identity, bounded reduction and delegation task/control authority |
| Depends on | Roadmap §Locked; JRN-4/JRN-7 for the process-death durability boundary |
| Proven by | Pure ledger, real-file authority and permit-backed runtime integration tests |

## Invariants

**COL-1 — One log, one ordered admission.** One stable collaboration identity owns contiguous,
one-based items, each binding one exact event; derived mail and delegation views rebuild from them.
Exact item retries return the original receipt before revision/capacity checks, while conflicting
item identities and sender-scoped mail identities reused under another item are refused unchanged.

**COL-2 — Admission is bounded.** Constructors, admission and replay enforce the bounds below,
including distinct artifact pointers and a mail-inaccessible control reserve.
Capacity refusal is explicit and changes nothing; accepted facts are never silently evicted.

**COL-3 — A delegated Conversation has one controller.** Creation fixes delegator and worker,
refuses cycles/reused workers and starts Main control. Only the fixed delegator may update the task
or complete Handoff at the exact current delegation revision; each mutation advances it,
Handoff is one-way, and later Main mutations are refused. A resolved admission is inspectable data,
not execution authority: the storage owner issues one non-cloneable permit under current Main
control, and Handoff requires every reservation or permit to be disposed. A permit is bound to one
physical authority and exact admission, can be issued only once, and retains writer authority until
it drops. After acknowledged Handoff, direct input requires a process-local owner-issued target for
the exact canonical worker; activation issues an exact runner-generation ticket and the runtime
rechecks current User control. A target or ticket from another owner or runner generation fails
before input; an exact durable Handoff retry returns its receipt
without interrupting User-owned work. Unknown Handoff writes freeze both Main and User authority
until reopen reconciles the canonical prefix. Slice 3 owns Main execution retention; SCH-2/SCH-4
own the bounded User input lane and its retained settlement.
Rejected: concurrent Main/user writers with precedence and objections, which required conflict
arbitration when an explicit handoff gives each input one owner.

**COL-4 — Acknowledgement follows the file append.** The exclusive writer validates and encodes,
then reduces the exact item only after successful unbuffered append and returns its receipt.
Write failure yields an unknown outcome and poisons the writer until reopen; even an old exact
retry cannot report success through that poisoned writer.

**COL-5 — Reopen validates a bounded prefix.** A distinct format/schema identifies the collaboration;
complete records undergo the same reduction checks as live admission before any tail repair.
Recovery follows the table below, writer/tail files are owner-only, and concurrent writers are
refused.

## Model

| Retained resource | Default / hard ceiling |
| --- | --- |
| Summary or task | Nonempty, at most 32 KiB UTF-8 each |
| Identity | Nonempty, at most 256 bytes |
| Artifact references per mail | At most 16 distinct conversation/artifact pairs |
| Total items | 4096 mail, task, Handoff and turn records; configurable downward |
| Delegations | 256; configurable downward |
| Semantic mail bytes | 16 MiB; configurable downward; text plus endpoint/pointer identities |
| Control reserve | 256 tail item slots unavailable to mail; configurable from zero to total items |

Mail addresses peers declared through delegation creation; declaration proves neither a session
file nor artifact availability. Item count also bounds retained control text and deduplication
indexes. The control reserve does not promise indefinite admission; the separate authority gate
permits one queued or active Main execution per delegation.

| Authority transition | Admission and resulting state |
| --- | --- |
| Creation | Revision zero, Main-controlled, attributed to the fixed delegator |
| Main task update | Fixed delegator and exact current revision; replace task and advance revision |
| Handoff | Fixed delegator, exact current revision and no retained execution reservation/permit; advance revision and become User-controlled |
| Main mutation after Handoff | Refuse; exact item retries still return their original receipt |

Authority applies to the whole Conversation. Partial-field merging, implicit transfer and return to
Main are not exposed. A ticket prepared before Handoff remains inspectable but cannot acquire a
permit afterward. Any unknown write freezes the entire writer authority until canonical reopen.

| Final file state | Reopen disposition |
| --- | --- |
| Complete, newline-terminated records | Validate and replay unchanged |
| Complete valid final JSON without newline | Retain its item and add newline |
| Incomplete final JSON prefix | Preserve the fragment in an owner-only sibling, then truncate |
| Truncated final UTF-8 character | Isolate only if its valid UTF-8 prefix is incomplete JSON |
| Impossible JSON prefix, invalid UTF-8, semantic error or earlier corruption | Fail closed without rewriting evidence |

COL-4 uses JRN-4's process-death boundary, with no fsync/power-loss guarantee. Replay performs no
provider, tool or UI effects.

## Evidence

| Invariant | Proven by |
| --- | --- |
| COL-1 | `col_1_exact_retry_and_replay_preserve_original_admission`, `col_1_mail_identity_and_endpoint_projection_are_canonical`, `col_4_file_roundtrip_retains_attribution_and_exact_retry` |
| COL-2 | `col_2_mail_saturation_preserves_control_admission`, `col_2_payload_boundaries_and_retained_bytes_are_enforced`, `col_2_exact_retention_limit_and_invalid_configuration`, `col_5_schema_and_decoded_bounds_fail_without_tail_repair` |
| COL-3 | `col_3_update_and_handoff_races_preserve_one_controller`, `col_3_wrong_authors_cycles_and_stale_tasks_are_refused`, `col_3_declared_endpoints_and_worker_ownership_cannot_be_rebound`, `col_3_execution_permit_blocks_handoff_and_old_ticket_dies_after_handoff`, `col_3_execution_slot_is_single_and_failed_bind_releases_it`, `col_3_handoff_serializes_new_execution_admission`, `col_3_ticket_from_another_file_cannot_cross_authority`, `cin_4_one_admission_cannot_issue_execution_twice_or_after_reopen`, `col_5_live_permit_does_not_keep_a_stale_control_open`, `col_3_unbound_delegated_runtime_fails_closed`, `col_3_permission_refresh_remains_available_under_main_control`, `col_3_main_control_gates_direct_input_until_handoff`, `col_3_permit_spans_barriers_and_two_main_rounds`, `col_3_stop_joins_main_owned_child_before_handoff`, `col_3_shutdown_joins_main_owned_child_and_releases_authority`, `col_3_failed_provider_join_releases_permit_after_cleanup`, `col_3_start_failure_releases_permit_after_cleanup`, `col_3_runtime_drop_joins_session_writer_before_permit_release`, `col_3_handoff_unlocks_only_the_authenticated_owned_child_input`, `col_3_idle_handoff_opens_user_input_until_owned_stop_begins`, `col_3_reopened_user_control_requires_explicit_activation_and_preserves_history` |
| COL-4 | `col_4_uncertain_append_recovers_every_byte_cut_and_retries_once`, `col_4_uncertain_handoff_recovers_before_any_execution_or_retry`, `col_4_uncertain_append_freezes_every_delegation_until_reopen`, `col_4_accepted_mail_survives_process_exit_without_drop`, `col_4_rejected_attempt_writes_nothing_and_does_not_poison` |
| COL-5 | `col_5_corruption_fails_closed_without_rewriting_evidence`, `col_5_schema_and_decoded_bounds_fail_without_tail_repair`, `col_5_exclusive_writer_and_owner_only_files`, `col_5_execution_permit_retains_writer_lock_until_disposed`, `col_5_execution_reservation_retains_writer_lock_until_disposed`, `col_5_idle_control_does_not_keep_a_closed_writer_locked`, `col_5_live_permit_does_not_keep_a_stale_control_open`, `col_4_uncertain_append_recovers_every_byte_cut_and_retries_once`, `col_4_uncertain_handoff_recovers_before_any_execution_or_retry`, `col_4_uncertain_append_freezes_every_delegation_until_reopen` |

## Integration boundary

Runtime child binding checks the exact worker Conversation and physical execution authority.
[SCH-1–SCH-5](./owned-scheduling.md) put the file and bounded child runners behind one composition
owner, reserve runner capacity before turn admission, retain permits through owned work, and join
Stop/Handoff. [CMP-1](./collaboration-mail-projection.md) derives attributed Incoming/Sent snapshots
through that live owner. [CTL-1](./collaboration-tools.md) seals Main authorship to the exact
user-owned root runtime tool ingress before constructing events; a serialized author is not a
credential. Production routes delegation and mail into both conversations; Stop routing and
controller/Handoff presentation remain unaccepted. Accepted admission means
present in this log, not included in a model request or completed. [CIN-1–CIN-4](./collaboration-inclusion.md)
own frozen turn admission, session inclusion references and the dispatch barrier. Provider
projection must preserve a distinct semantic mail atom, with an explicit encoding or typed refusal
per dialect; this component proves no endpoint support.
