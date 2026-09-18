# Spec — Collaboration mail projection

| Field | Value |
| --- | --- |
| Status | Implemented. CMP-1 is wired and accepted: `scripts/smoke-delegate.py` draws the letter in both conversations. CMP-2's session join has no caller, so queued and included still read the same |
| Owns | Read-only Incoming/Sent mail views and recipient-session inclusion over one canonical collaboration log |
| Depends on | COL-1/COL-2/COL-4; the mail rules in [`ui-ux.md`](../ui-ux.md) §product areas |
| Proven by | Pure ledger and live-writer/reopen tests named below |

## Invariants

**CMP-1 — Mail views are canonical snapshots.** A known endpoint receives one complete bounded
snapshot of its incoming and sent `MailAccepted` records in ascending collaboration sequence. Each
item retains its collaboration reference, both attributed endpoints, summary and artifact pointers;
while its canonical prefix is provable, the live asynchronous owner returns the same snapshot as
reopen and never copies mail through a session transcript. An uncertain writer refuses projection
until reopen instead of returning its stale in-memory prefix.

**CMP-2 — Inclusion comes only from the recipient session.** One writer observation joins the
canonical mail snapshot with exact collaboration admissions referenced by a selected session
branch sealed by that live runtime's exact collaboration capability. Raw journals and sources from
another owner or runtime instance are refused. A dedicated bounded child inspection lane remains
available during active work without consuming Stop capacity. Each reference is validated against
its persisted `CollaborationTurnStarted` boundary.
Incoming mail is `Queued` until one such admission includes its canonical item reference, then
`Included` with that admission reference. Sent mail is `OtherRecipient` because this projection
does not inspect the recipient's session. No state implies consumption, seen state or Attention.

## Evidence

| Invariant | Proven by |
| --- | --- |
| CMP-1 | `cmp_1_mail_projection_merges_both_directions_in_first_appearance_order`, `cmp_1_owned_mail_snapshot_equals_the_reopened_projection`, `col_4_file_roundtrip_retains_attribution_and_exact_retry`, `col_4_accepted_mail_survives_process_exit_without_drop`, `col_4_uncertain_append_freezes_every_delegation_until_reopen` |
| CMP-2 | `cmp_2_session_mail_joins_exact_inclusion_and_leaves_sent_status_remote`, `cmp_2_active_owned_child_projects_session_mail_without_blocking_stop`, CIN-2 evidence |

## Integration boundary

`TurnAdmitted` alone still proves no model inclusion; CMP-2 requires an exact-runtime-sealed snapshot of
the recipient session's branch-local `CollaborationTurnStarted` fact. Product routing consumes CMP-1 —
`scripts/smoke-delegate.py` draws each letter in both conversations — and has not consumed the CMP-2
join, so queued and included still read the same on screen.
This mechanism does not infer Attention from mail. ATT-1–ATT-3 use distinct reference-only
collaboration records joined to the producer journal; mail, task updates and Handoff remain
ineligible substitutes. Seen or acknowledged presentation state is separate from canonical
resolution.
