# Spec — Collaboration tool contract

| Field | Value |
| --- | --- |
| Status | Typed grammar, runtime-sealed ingress, recoverable child provisioning and artifact resolution implemented; production provider activation unproven |
| Owns | Exact model-visible delegation, mail, task-update and Handoff schemas before authority binding |
| Depends on | COL-1–COL-3; CHB-1; PRV-1; the typed-mail and single-controller rules in the roadmap |
| Proven by | Runtime schema and parsing tests named below |

## Invariants

**CTL-1 — Model arguments carry intent, never authority.** `delegate`, `send_mail`, `update_task`
and `handoff` use closed bounded JSON objects. A Main call may carry only an opaque owner-issued
`target` selector; artifact pointers are opaque sender-scoped selectors. No argument can supply an
author, sender/recipient endpoint, Conversation, canonical delegation, collaboration item, mail
identity, revision or retry identity. Authenticated owner ingress resolves selectors and derives
all canonical facts from its current registry. Its bounded lane retains an accepted command and
owner-generated attempt across caller cancellation and shutdown. Main authorship is issued only by
the user-owned root runtime carrying that exact ingress and a unique runtime-instance token; a raw
endpoint or another runtime built from a cloned catalog cannot bind or project as it. Once
canonical delegation creation is acknowledged, any later provisioning failure returns the exact
opaque target and retains its detailed cause so explicit recovery cannot create a second delegation.

**CTL-2 — Tool visibility follows controller role.** Main has all four provider-neutral
definitions. A delegated child has only `send_mail`, implicitly addressed to its fixed delegator;
it cannot delegate, update a task or hand off control. Main and child mail use distinct trusted
definition identities because their schemas and authenticated endpoints differ. The parser rejects
duplicate artifact selectors before admission. Versioned artifact selectors derive from immutable
sender journal facts projected by the runtime carrying the exact owner authority. A raw journal,
another owner, or a child capability from another canonical provenance cannot register them;
unknown, foreign or ambiguous facts fail atomically before collaboration admission.

## Evidence

| Invariant | Proven by |
| --- | --- |
| CTL-1 | `ctl_1_collaboration_tool_schemas_are_exact_scoped_and_authority_free`, `ctl_1_arguments_cannot_supply_authority_or_escape_semantic_bounds`, `ctl_1_admission_canonicalizes_and_rechecks_role_definition_and_capability`, `ctl_1_writer_resolves_current_delegation_state_for_owner_ingress`, `ctl_1_ingress_derives_mail_endpoints_and_current_task_revision`, `ctl_1_main_identity_is_issued_by_the_exact_root_catalog`, `cmp_2_session_mail_joins_exact_inclusion_and_leaves_sent_status_remote`, `ctl_1_ingress_refuses_unknown_targets_and_unbound_artifacts_without_mutation`, `ctl_1_cancelled_tool_wait_does_not_cancel_an_accepted_ingress_mutation`, `ctl_1_child_mail_cancellation_releases_the_caller_and_retains_the_mutation`, `ctl_1_child_stop_does_not_deadlock_behind_its_accepted_mail`, `ctl_1_ingress_lane_is_bounded_and_shutdown_retains_all_settlements`, `ctl_1_restart_rebuilds_targets_without_exposing_durable_identities`, `ctl_1_artifact_selector_requires_an_authenticated_sender_journal_fact`, `ctl_1_ambiguous_artifact_identity_refuses_before_collaboration_admission`, `ctl_1_unsupported_provider_refuses_before_child_or_delegation_creation`, `ctl_1_cancelled_delegate_wait_settles_once_without_duplicate_creation`, `ctl_1_explicit_resume_recovers_a_canonical_child_missing_its_journal`, `ctl_1_post_canonical_provisioning_failure_returns_target_for_explicit_resume`, `ctl_1_post_build_registration_failure_cleans_up_before_explicit_resume`, `ctl_1_delegate_preflights_capacity_and_resumes_without_duplicate_creation`, `ctl_1_main_mail_wakes_an_engaged_child_for_a_fresh_second_turn`, `ctl_1_root_activity_multiplexes_late_ingress_without_polling`, `collaboration_definition_ids_remain_unique_and_role_specific` |
| CTL-2 | `ctl_1_collaboration_tool_schemas_are_exact_scoped_and_authority_free`, `ctl_2_mail_target_is_role_derived_and_typed`, `ctl_1_ingress_derives_mail_endpoints_and_current_task_revision`, `ctl_2_catalog_rechecks_role_and_handoff_uses_owner_derived_revision` |

## Integration boundary

The role-aware catalog admits only an explicitly bound Main or child capability and rechecks its
definition identity, revision, capability and role before sending a command. The owner derives
mail endpoints and current task/Handoff revisions. Versioned SHA-256 selectors derive from complete
immutable target/artifact origins and expose no canonical IDs. A target registry rebuilds from
canonical delegation creation on resume; artifact registration requires a runtime-sealed immutable
fact from the exact registered runtime instance on the sender's selected journal branch and stages
the complete registry change before mutation.
`delegate` preflights deterministic failures before allocating an identity, then writes canonical
creation before creating the child journal. Post-creation failure returns the exact target; an
explicit resume reuses the canonical worker and journal without another creation or automatic
wake. In-process cancellation is proven to settle that exact attempt once; canonical-only
provisioning survives reopen and recreates its missing child without a duplicate, while a real
process kill at that exact window remains unproven. The current four
production adapters all fail provider preflight; a synthetic supporting driver proves creation,
registration, repeated Main-to-child wake and target return without enabling production execution.
