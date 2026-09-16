# Spec — Delegated child bootstrap

| Field | Value |
| --- | --- |
| Status | Implemented, wired and accepted. The real child stays passive across graceful and process-kill resume; an old approval cannot continue and fresh work uses current policy |
| Owns | Read-only child capability floor, shared coding Session policy, child journal placement and bound fresh/resumed runtime construction |
| Depends on | COL-3; CIN-2/CIN-4; APV-1–APV-3; JRN-3–JRN-5; PRV-6 |
| Proven by | Native catalog, real-file provenance/directory and direct runtime-constructor tests |

## Invariants

**CHB-1 — Child capabilities have a hard floor.** A V1 delegated child always has native
`read_file` and `search`, subject to the shared coding Session policy in PER-1. When the root binds
an authenticated collaboration owner, it adds only
the child-specific typed `send_mail` definition, whose recipient is fixed by canonical provenance.
Skills, file mutation, command execution, delegation, task control, permission memory and prompt
text cannot widen that set; execution rechecks the profile even for a call admitted by another
catalog.

**CHB-2 — Child construction is exact and closed.** Fresh and resumed child constructors require
one binding whose collaboration, delegation, worker Agent and child Conversation identities agree
before the runtime is exposed. The child mail catalog is crate-private and must carry the same full
canonical provenance as the runtime binding. They force CHB-1 and accept only a `ResolvedModel`, whose configured
reasoning effort has already passed model-local validation. Production construction installs the
root coding Session's permission owner before the child is exposed. Control is installed before any
bootstrap or recovery append; a fresh constructor refuses a journal that already has records.
Explicit User activation uses the same resumed constructor and fixed catalog after canonical User
control and the current owner's process-local target have both been validated. It requires the
existing delegated journal; activation cannot create missing child history.

**CHB-3 — Root resume never activates a child.** User-selectable root journals and delegated child
journals have separate owner-only directories that issue distinct, non-interconvertible runtime
tokens. Root constructors accept only a root token and delegated constructors accept only a child
token. Child history activates only through the delegated path and CHB-2; recovery dispatches no
provider or tool, and interrupted work needs a fresh explicit submission rather than automatic
restart. A process-dead approval has no decision route and cannot continue; a later explicit task
submission starts new model work under current policy. Explicit target resume may reconstruct a
canonical Main-controlled child but does not wake it.
User-controlled reconstruction requires a separately issued User target and explicit activation;
it likewise sends no input, provider request or tool effect.

## Model

```text
root selection ──▶ root conversations/ ──▶ user runtime

collaboration owner ──▶ delegated-sessions/ ──▶ storage-issued journal token
                                                └─ exact canonical binding
                                                   ├─ read/search + fixed-parent mail profile
                                                   └─ child runtime
```

Directory tokens close runtime routing, while COL-3 owns durable Controller and execution authority.
A raw path or journal cannot enter a public runtime constructor. Handoff changes input ownership and
does not change the CHB-1 profile.

## Evidence

| Invariant | Proven by |
| --- | --- |
| CHB-1 | `chb_1_read_only_catalog_cannot_be_widened_by_an_admitted_call`, `chb_2_fresh_child_constructor_is_exact_and_forces_read_only_tools`, `ctl_1_ingress_derives_mail_endpoints_and_current_task_revision`, `ctl_2_catalog_rechecks_role_and_handoff_uses_owner_derived_revision`; `scripts/smoke-delegate.py` uses the production child inspection source |
| CHB-2 | `chb_2_delegated_control_retains_all_creation_provenance_across_reopen`, `chb_2_fresh_child_constructor_is_exact_and_forces_read_only_tools`, `ctl_1_delegate_preflights_capacity_and_resumes_without_duplicate_creation`, `provisioning_process_death_recovers_one_exact_passive_child`, `col_3_reopened_user_control_requires_explicit_activation_and_preserves_history`, `chb_3_user_activation_requires_the_existing_delegated_journal`; `scripts/smoke-delegate.py` proves the child shares root startup policy |
| CHB-3 | `chb_3_delegated_journals_require_their_explicit_directory`, `chb_3_resumed_child_settles_interruption_without_redispatch`, `ctl_1_explicit_resume_recovers_a_canonical_child_missing_its_journal`, `ctl_1_delegate_preflights_capacity_and_resumes_without_duplicate_creation`, `provisioning_process_death_recovers_one_exact_passive_child`, `col_3_reopened_user_control_requires_explicit_activation_and_preserves_history`, `chb_3_user_activation_requires_the_existing_delegated_journal`, `missing_resumed_child_history_projects_one_explicit_unavailable_state`, `locked_resumed_child_history_projects_one_explicit_unavailable_state`, `corrupt_resumed_child_history_projects_one_explicit_unavailable_state`; `scripts/smoke-delegate.py` proves graceful and process-kill passive pointer/keyboard browsing with no request or durable write, including a killed child request whose old approval cannot continue and whose next explicit task uses current policy |

## Integration boundary

[SCH-1–SCH-5](./owned-scheduling.md) validate canonical provenance, restore selected-branch
collaboration context and expose a bounded runner while keeping child history out of ordinary root
selection. The storage token proves directory origin; canonical provenance and COL-3 prove runtime
authority. Passive UI projection opens the validated child journal without constructing a runtime;
missing, writer-locked or invalid evidence keeps its roster row and projects one explicit warning.
Main-controlled child tree/compaction routing remains product work. Provider collaboration encoding
is bounded by PRV-1's four explicit dialects and makes no live-endpoint claim.
