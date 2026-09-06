# Spec — Personal project permission store

| Field | Value |
| --- | --- |
| Status | Implemented locally; store, runtime, configuration and executable evidence reviewed |
| Owns | Physical identity, bounded personal storage and cross-process transactions |
| Depends on | [permission-policy](./permission-policy.md) PER-4–PER-6 |
| Proven by | Store process/fault tests, runtime PER-6 tests and PER-8/PER-10 executable journey |

## Invariants

**PGR-1 — Physical project identity.** The namespace binds the discovered canonical project path,
its device and inode. Each transaction revalidates the pinned project and personal-store paths;
replaced roots, linked permission paths and non-private child directories cannot supply authority.

**PGR-2 — One transaction order.** Readers, mutations and dispatch refreshes take a bounded,
cancellable exclusive lock on one stable file. Mutations compare both store identity and sequence;
reset creates a fresh identity, so old revisions cannot match again.

**PGR-3 — Validate the whole source.** Every record, sequence, grant identity and constructor must
validate within explicit bounds before any snapshot is returned. A malformed, unsupported or torn
source is a typed failure, including a complete final JSON value without its newline.
Rejected: Conversation valid-prefix recovery, because a discarded suffix may contain a revoke.

**PGR-4 — Acknowledgement follows persistence.** Mutations acknowledge only after file sync;
initialization/reset also sync the containing directory. A failed/unknown write returns no new
authority; mutation consumes the transaction, preventing reuse of an older snapshot after failure.

**PGR-5 — Personal policy remains separate.** Grants, revokes and exact project-config trust live
under `PLEXMATON_HOME/projects/<physical-key>/permissions.jsonl`, outside Conversation history and
project configuration. An initialized stable lock plus a missing log is corruption, never absence.

## Storage

The stable `permissions.lock` is never replaced. Its bounded initialization marker distinguishes an
absent, never-written source from a deleted log; it holds no grants. `permissions.jsonl` begins with
format 1, physical project identity and a fresh store identity, followed by grant/revoke/trust records.
A grant identity cannot be reused after revocation within that store. Trust records name an exact
SHA-256 configuration fingerprint and confer no authority through loaded skills or model selection.

Limits: 128 active grants, 4096 mutations, 192 KiB per encoded record, 16 MiB per whole source.
Project offers require room for a maximum-sized grant as well as free count and revision capacity.
Exhaustion refuses mutation; no automatic compaction or repair restores an older allow. Reset is
explicit and accepts only a healthy source with its current revision. Cancellation before writing
applies nothing; cancellation after writing begins cannot roll back a committed grant.

Private project directories use mode 0700; regular, singly-linked log/lock files use 0600 and the
current effective UID. Files open relative to pinned descriptors with no symlink following and
nonblocking type validation. Local Unix filesystems are the target; hostile same-user mutation,
network filesystem lock semantics and power-loss behavior beyond host sync guarantees are unproven.

## Evidence

| Invariant | Evidence |
| --- | --- |
| PGR-1 | `pgr_1_aliases_share_identity_but_distinct_and_replaced_roots_do_not`, `pgr_1_private_permissions_and_pinned_parents_reject_replacement`, `pgr_1_symlinks_hardlinks_and_replaced_locks_supply_no_authority` |
| PGR-2 | `pgr_2_two_process_writers_commit_once_and_never_merge_stale_grants`, `pgr_2_stale_writes_and_reset_cannot_recreate_an_old_revision`, `pgr_2_dispatch_after_another_process_revokes_observes_the_revoke`, `pgr_2_a_process_waits_for_authorization_lock_then_observes_current_policy`, `pgr_2_cancelled_wait_and_mutation_apply_nothing`, `pgr_2_process_death_releases_the_stable_lock_and_torn_writes_stay_refused`, `per_6_project_command_survives_restart_and_dispatch_observes_external_revoke` |
| PGR-3 | `pgr_3_torn_complete_without_newline_and_invalid_records_never_restore_a_prefix`, `pgr_3_format_binding_and_resource_bounds_refuse_the_whole_source`, `pgr_3_active_grant_and_retained_mutation_limits_do_not_partially_apply`, `per_4_permission_wire_scopes_validate_the_same_constructors`, `per_6_corrupt_project_source_refuses_allow_once_before_its_effect` |
| PGR-4 | `pgr_4_failed_partial_and_unknown_writes_publish_no_success`, `pgr_4_lost_grant_ack_retains_the_durable_grant_without_reporting_success`, `per_6_project_grant_saved_then_conversation_audit_failed_starts_no_effect` |
| PGR-5 | `pgr_5_absence_grants_trust_and_revocation_have_one_personal_source`, `per_8_project_allow_requires_exact_personal_trust_and_edits_invalidate_the_review`, `per_8_trusted_configuration_runs_the_command_and_dispatch_rechecks_changed_bytes` |
