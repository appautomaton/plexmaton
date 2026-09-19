# Evidence — Delegated child bootstrap

What proves [delegated-bootstrap](../specs/delegated-bootstrap.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| CHB-1 | `chb_1_read_only_catalog_cannot_be_widened_by_an_admitted_call`, `chb_2_fresh_child_constructor_is_exact_and_forces_read_only_tools`, `ctl_1_ingress_derives_mail_endpoints_and_current_task_revision`, `ctl_2_catalog_rechecks_role_and_handoff_uses_owner_derived_revision`; `scripts/smoke-delegate.py` uses the production child inspection source |
| CHB-2 | `chb_2_delegated_control_retains_all_creation_provenance_across_reopen`, `chb_2_fresh_child_constructor_is_exact_and_forces_read_only_tools`, `ctl_1_delegate_preflights_capacity_and_resumes_without_duplicate_creation`, `provisioning_process_death_recovers_one_exact_passive_child`, `col_3_reopened_user_control_requires_explicit_activation_and_preserves_history`, `chb_3_user_activation_requires_the_existing_delegated_journal`; `scripts/smoke-delegate.py` proves the child shares root startup policy |
| CHB-3 | `chb_3_delegated_journals_require_their_explicit_directory`, `chb_3_resumed_child_settles_interruption_without_redispatch`, `ctl_1_explicit_resume_recovers_a_canonical_child_missing_its_journal`, `ctl_1_delegate_preflights_capacity_and_resumes_without_duplicate_creation`, `provisioning_process_death_recovers_one_exact_passive_child`, `col_3_reopened_user_control_requires_explicit_activation_and_preserves_history`, `chb_3_user_activation_requires_the_existing_delegated_journal`, `missing_resumed_child_history_projects_one_explicit_unavailable_state`, `locked_resumed_child_history_projects_one_explicit_unavailable_state`, `corrupt_resumed_child_history_projects_one_explicit_unavailable_state`; `scripts/smoke-delegate.py` proves graceful and process-kill passive pointer/keyboard browsing with no request or durable write, including a killed child request whose old approval cannot continue and whose next explicit task uses current policy |
