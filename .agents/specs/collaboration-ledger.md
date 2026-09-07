# Spec — Collaboration ledger

| Field | Value |
| --- | --- |
| Status | Admission implemented; runtime integration unproven |
| Owns | Canonical collaboration admission, retry identity, bounded reduction and delegation task authority |
| Depends on | Roadmap §Locked; JRN-4/JRN-7 for the process-death durability boundary |
| Proven by | Pure ledger tests and real-file admission/recovery tests |

## Invariants

**COL-1 — One log, one ordered admission.** One stable collaboration identity owns contiguous,
one-based items, each binding one exact event; derived mail and delegation views rebuild from them.
Exact item retries return the original receipt before revision/capacity checks, while conflicting
item identities and sender-scoped mail identities reused under another item are refused unchanged.

**COL-2 — Admission is bounded.** Constructors, admission and replay enforce the bounds below,
including distinct artifact pointers and a mail-inaccessible control reserve.
Capacity refusal is explicit and changes nothing; accepted facts are never silently evicted.

**COL-3 — A delegation has attributed task authority.** Creation fixes delegator and worker,
refuses cycles and reused workers, and binds one agent identity per declared conversation.
Only the delegator or user may amend the task under the authority transitions below; objections
retain authorship without changing the task, and no authority is inferred from prose.
Rejected: treating an agent's observation of the current revision as permission to overwrite a
user-authored task; observing an instruction does not authorize undoing it.

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
| Summary, task or objection | Nonempty, at most 32 KiB UTF-8 each |
| Identity | Nonempty, at most 256 bytes |
| Artifact references per mail | At most 16 distinct conversation/artifact pairs |
| Total items | 4096; configurable downward |
| Delegations | 256; configurable downward |
| Semantic mail bytes | 16 MiB; configurable downward; text plus endpoint/pointer identities |
| Control reserve | 256 tail item slots unavailable to mail; configurable from zero to total items |

Mail addresses peers declared through delegation creation; declaration proves neither a session
file nor artifact availability. Item count also bounds retained control text and deduplication
indexes. The control reserve does not promise indefinite admission or per-operation completion
capacity.

| Authority transition | Admission and resulting task |
| --- | --- |
| Creation | Revision zero, authored by the fixed delegator |
| Delegator amendment | Exact current revision and no prior user task ownership; advance revision |
| User amendment | May rebase over agent edits, but not an unseen user edit or future revision; advance revision and retain user ownership |
| Agent edit after user ownership | Refuse even at the current revision |
| Delegator objection | Exact task revision; append attributed summary, leave task/revision unchanged |

Authority applies to the whole task. Partial-field merging and explicit return of authority are
not exposed. A later user edit must have observed the latest user-authored revision.

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
| COL-2 | `col_2_mail_saturation_preserves_control_admission`, `col_2_payload_boundaries_and_retained_bytes_are_enforced`, `col_2_exact_retention_limit_and_invalid_configuration` |
| COL-3 | `col_3_user_wins_both_writer_orders_and_objections_preserve_task`, `col_3_wrong_authors_cycles_and_stale_tasks_are_refused`, `col_3_declared_endpoints_and_worker_ownership_cannot_be_rebound` |
| COL-4 | `col_4_uncertain_append_recovers_every_byte_cut_and_retries_once`, `col_4_accepted_mail_survives_process_exit_without_drop`, `col_4_rejected_attempt_writes_nothing_and_does_not_poison` |
| COL-5 | `col_5_corruption_fails_closed_without_rewriting_evidence`, `col_5_schema_and_decoded_bounds_fail_without_tail_repair`, `col_5_exclusive_writer_and_owner_only_files`, `col_4_uncertain_append_recovers_every_byte_cut_and_retries_once` |

## Integration boundary

Runtime ingress authentication is unproven: the runtime must establish authorship before
constructing events; a serialized author is not a credential. Accepted means present in this log,
not included in a model request or completed. Session inclusion must later retain a canonical item
reference and exact execution boundary. The delegator's next turn must include all effective
amendments or be held; this barrier remains unproven.

Scheduling, stop policy, depth/concurrency bounds and per-operation terminal capacity reservations
are not implemented here. Provider projection must preserve a distinct semantic mail atom, with an
explicit encoding or typed refusal per dialect; this component proves no endpoint support.
