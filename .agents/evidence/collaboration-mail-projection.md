# Evidence — Collaboration mail projection

What proves [collaboration-mail-projection](../specs/collaboration-mail-projection.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| CMP-1 | `cmp_1_mail_projection_merges_both_directions_in_first_appearance_order`, `cmp_1_owned_mail_snapshot_equals_the_reopened_projection`, `col_4_file_roundtrip_retains_attribution_and_exact_retry`, `col_4_accepted_mail_survives_process_exit_without_drop`, `col_4_uncertain_append_freezes_every_delegation_until_reopen` |
| CMP-2 | `cmp_2_session_mail_joins_exact_inclusion_and_leaves_sent_status_remote`, `cmp_2_active_owned_child_projects_session_mail_without_blocking_stop`, CIN-2 evidence |
