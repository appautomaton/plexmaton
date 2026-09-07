# Spec — Collaboration turn inclusion

| Field | Value |
| --- | --- |
| Status | Narrow collaboration-turn path implemented; production orchestration unproven |
| Owns | Turn-admission ordering, canonical session references and typed context resolution |
| Depends on | Roadmap §Locked; COL-1–COL-5; JRN-1/JRN-7; TIM-2 |
| Proven by | Ledger/session tests, real-file scripted runtime tests and provider refusal fixtures |

## Invariants

**CIN-1 — Admission freezes the eligible prefix.** A TurnAdmitted item pins recipient, proposed
session head/boundary and turn identity, previous included admission, and all eligible source items
in order up to its own log position. An amendment accepted before that position must be included;
one accepted later cannot change the frozen turn and remains eligible for a later admission.

**CIN-2 — Inclusion is a session fact.** One collaboration turn-start atomically records the turn
boundary and canonical admission reference without copying mail/task bodies or creating user input.
The previous-inclusion cursor comes from selected session ancestry, so admission without a session
record does not consume mail; missing, foreign and duplicate references fail closed.

**CIN-3 — Resolution preserves attribution and revision.** A resolved context atom contains the
exact immutable admitted source items and their task revisions at those source positions, with
bounded transient retention. A provider codec lacking the representation refuses explicitly; neither
unresolved references nor resolved mail are converted into user-role text.

**CIN-4 — Session acknowledgement precedes driver dispatch.** The collaboration-turn path publishes
no model call before its session inclusion and request authorization appends are acknowledged;
cancelled outer waits retain the owned transition, and uncertain persistence requires reopen.
Replay reconstructs references/context without automatically running providers or tools.

## Model

TurnAdmitted is the logical ordering point in the collaboration log. The later session record
establishes actual inclusion. If the process stops between those writes, a new explicit admission
uses the session's previous included reference and therefore sees the still-pending source items.
Retrying the original item identity resolves that exact frozen admission; it is not permission to
start a second execution. Session boundary validation prevents a stale admission from opening over
an advanced head. A new explicit execution needs a fresh turn/admission identity.

The initial eligible set includes mail to the recipient, delegation creation/task amendments for
either participant, and objections for the delegator. At most 64 source items and 256 KiB semantic
source bytes enter one admission; overflow holds admission rather than truncating amendments.
A transient resolved context cache is capped at 256 admissions and 16 MiB semantic source bytes,
including variable author identities; fixed container overhead is bounded separately by counts. It
is reconstructed from canonical references, never serialized as another task or mail authority.
The runtime path requires an attached journal and explicit driver support. Unsupported drivers or
unavailable historical materialization refuse before a new collaboration turn is written. A later
preparation failure settles the un-dispatched step through the existing turn terminal path.

## Evidence

| Invariant | Proven by |
| --- | --- |
| CIN-1 | `cin_1_admission_orders_amendments_and_freezes_original_revision`, `cin_1_source_capacity_holds_the_turn_without_advancing_log`, `cin_1_source_bytes_include_attributed_agent_identities`, `cin_2_reopen_between_logs_keeps_unincluded_items_pending` |
| CIN-2 | `cin_2_automatic_journal_materializes_first_collaboration_turn`, `cin_2_unincluded_admission_and_branch_retain_pending_sources`, `cin_3_session_reference_resolves_without_synthetic_user_content`, `cin_2_reopen_between_logs_keeps_unincluded_items_pending`, `cin_4_uncertain_inclusion_reopens_without_redispatch` |
| CIN-3 | `cin_3_session_reference_resolves_without_synthetic_user_content`, `cin_3_resolved_cache_is_bounded_and_exact_reinsertion_is_free`, `cin_3_resolved_cache_byte_cap_is_independent_of_turn_count`, `cin_3_all_codecs_refuse_collaboration_context_explicitly`, `cin_3_unsupported_driver_refuses_before_session_mutation` |
| CIN-4 | `cin_4_inclusion_and_request_authorization_each_gate_dispatch`, `cin_4_cancelled_start_retains_inclusion_until_acknowledgement`, `cin_4_uncertain_inclusion_reopens_without_redispatch`, `cin_4_unresolved_history_never_strands_an_authorized_step`, `cin_4_two_scripted_runtimes_progress_and_stop_independently` |

## Integration boundary

Production runner ownership, all ordinary user-input turn-opening paths, admission authentication,
completion capacity, automatic wake and UI projection remain unproven. This mechanism does not
promise exactly-once model/tool effects or power-loss durability. Branch/export/delete must retain
referenced collaboration logs; unsupported resolution blocks continuation.
