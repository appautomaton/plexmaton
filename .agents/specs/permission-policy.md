# Spec — Permission policy

| Field | Value |
| --- | --- |
| Status | Implemented locally; component, executable and three-width evidence reviewed |
| Owns | Reusable permission scope, precedence and coding Session/project authority |
| Proven by | Reducer, runtime, store, configuration, parser, TUI and executable proofs below |
| Depends on | [tool-admission](./tool-admission.md) APV-1–APV-6; [session-journal](./session-journal.md) JRN-7 |

## Invariants

**PER-1 — Session authority outlives a Conversation.** One explicit owner retains temporary
grants for the coding Session and physical workspace. Opening a new or saved conversation, head selection and
compaction do not reset it. Exit/restart creates fresh memory-only authority; Conversation JSONL
never restores a grant. Immutable snapshots are projections of this owner.

**PER-2 — Explicit rules precede memory.** Deny precedes Ask, then Allow or a remembered grant,
then capability fallback. Native read/search fallback allows; native writes and process spawning
ask. A reusable offer is valid only when its addition authorizes the whole admitted operation;
explicit Ask/Deny cannot be bypassed by remembering it. APV-3 still applies.

**PER-3 — A preset names definitions.** The native file-change preset pins create and edit
identities/revisions and excludes agent-control/configuration paths and Git metadata. Neither a
`FileWrite` capability nor a similar display name makes another tool a member. Typed permission
subjects are issued by the trusted catalog through its APV-1 ticket, never reconstructed from
approval detail.

**PER-4 — Reuse retains scope and revision.** Exact command grants match exact source, definition,
revision and captured execution context, including the physical working directory, shell and
filtered environment. A changed context asks again. Mutations echo the reviewed owner/revision;
stale, foreign, capacity-exhausted and ineffective requests leave the prior state intact.

**PER-5 — A decision has a commit boundary.** The current pending Conversation/head/turn/call and
admitted arguments are checked before applying a user decision. JRN-7 acknowledges its audit
before dependent execution. Dispatch rechecks current policy authority; revoked or stale reusable
permissions cannot start an effect. Pending decisions remain visible until producer confirmation.
An interrupt or shutdown accepted before dispatch prevents a worker from starting, even while the
preceding audit is blocked. A failed audit does not implicitly revoke an already applied grant.

**PER-6 — Persistent project grants are personal policy.** Project grants belong under the
Plexmaton user root and bind physical project identity. They are separate from Conversation JSONL
and project-controlled configuration. Corrupt/torn records, stale revisions and failed/unknown
writes cannot authorize execution. A durable project grant is not rolled back if the subsequent
Conversation audit fails; the runtime reports the saved grant and that the tool did not run.
[Project storage](./project-permissions.md) PGR-1–PGR-5 owns its transaction protocol.
Cancellation also retains a completed Project worker receipt when another Conversation audit
fails before that worker is consumed. Feedback follows the affected call, changes no semantic
copy or item revision, and survives terminal release. A required invalid source returns typed
`Unavailable`, including for Allow once.
PER-8 defines configured authority and personal trust.

**PER-7 — Controls change grants, not parallel settings.** The native file-change setting derives
from its named grant; disabling it revokes that identity without promising that other sources
will stop allowing matching operations. Review and Back apply nothing, submission waits for
producer confirmation, and stale controls return the current view. Enabling a Session setting
before the first turn creates no Conversation JSONL. A completed change re-evaluates covered
waiting calls through PER-5. Controls sit where their lifetime is typed: the Session's grants
and the native preset are `/permissions` rows in the composer menu (CMC-3); Project grants and
configuration trust are the Drawer's Permissions page. A grant is offered in one place only, and
one acknowledged view lands in every open place.

**PER-8 — Configuration trust names exact bytes.** User rules are a startup snapshot. Both user
and project readers use one strict bounded declaration grammar compiled by the trusted catalog.
Project Ask/Deny apply without trust; Project Allow requires a personal trust record matching the
complete current file's SHA-256 fingerprint. Every project transaction refreshes the file before
mutation or execution. Changed bytes invalidate the reviewed revision; invalid sources return
Unavailable. Model and skill loading confer no trust. Trust review exposes every configured scope
in a scrollable view; confirmation echoes the fingerprint and current permission revision.

**PER-9 — Decision history is evidence, never authority.** Each admitted policy decision and
completed user choice records its call, definition/revision, observed Session/project revision,
command-context fingerprint and winning rule or grant scope before dependent effects. A choice
also retains its approval identity, reason and remembered grant identity. Replay validates call
ownership and lifecycle, adds no model content, and installs no permissions. This records the
reducer's decision; a later dispatch refusal remains a separate typed tool outcome. Configuration
fingerprints and context hashes retain provenance without copying whole policies or environment
values. At most one decision per queued/awaiting boundary is accepted.

**PER-10 — Prefix authority covers complete literal operations.** A maintained parser at command
admission lowers only bounded complete literal sequences, preserving argv boundaries, source spans
and PER-4 context. Prefix Allow/grants must cover every command; prefix Ask/Deny applies to any
covered command before exact or reusable Allow. Unsupported syntax never matches a prefix.
Suggestions name meaningful floors (`ls`, `git fetch`), never discard leading options or wrappers,
and are offered only when effective for the whole call. Exact fallback explains why no prefix is
offered. This classifies syntax and scope, not executable effects or OS confinement (CMD-2).
On short cards the complete scope precedes repeated operation detail and explanatory notes. A
clipped scope disables remembered grants for both keyboard and pointer input; Back remains usable.

## Evidence

| Invariant | Evidence |
| --- | --- |
| PER-1 | `per_1_memory_is_owned_by_the_coding_session_and_snapshots_cannot_mutate_it`, `per_1_remembered_command_survives_runtime_replacement_and_revocation_restores_asking`, `new_session_is_lazy_and_replacement_preserves_saved_history`, `per_1_coding_session_authority_expires_on_restart_and_refuses_other_workspaces` |
| PER-2 | `per_2_deny_then_ask_precede_allow_and_memory_in_every_rule_order`, `per_2_explicit_capability_ask_cannot_hide_a_matching_deny_or_be_remembered`, `per_5_remember_releases_covered_waiting_siblings_through_current_policy` |
| PER-3 | `per_3_file_change_preset_pins_definitions_and_excludes_control_paths`; existing native admission/executor tests remain APV-3 evidence |
| PER-4 | `per_4_exact_commands_preserve_source_and_context_without_reading_detail`, `per_4_stale_foreign_and_full_mutations_preserve_current_authority`, `per_4_a_changed_policy_reissues_choices_without_applying_the_stale_decision`, `per_4_changed_offer_returns_to_review_and_back_never_grants`, `per_4_command_subject_tracks_executor_context_and_preserves_exact_source` |
| PER-5 | `per_5_remember_prepares_once_and_only_then_produces_the_audited_execution`, `per_5_preparation_failure_keeps_the_request_and_cancellation_refuses_late_completion`, `per_5_allow_once_cannot_cross_conversations_with_reused_provider_call_ids`, `per_5_remember_is_two_steps_and_submission_disables_duplicate_decisions`, `attention_keyboard_activates_the_visible_worker_and_escape_restores_primary_card`, `per_5_failed_remember_audit_never_dispatches_the_prepared_command`, `per_5_revocation_between_preparation_and_dispatch_refuses_the_effect`, `per_5_interrupt_before_permission_audit_ack_starts_no_worker`, `per_5_shutdown_before_permission_audit_ack_starts_no_worker`, `per_5_remembered_scope_frames_preserve_the_operation_and_composer` |
| PER-6 | `per_6_project_observations_invalidate_offers_and_unavailable_sources_never_allow`, `per_6_project_command_survives_restart_and_dispatch_observes_external_revoke`, `per_6_corrupt_project_source_refuses_allow_once_before_its_effect`, `per_6_project_grant_saved_then_conversation_audit_failed_starts_no_effect`, `per_6_project_receipt_survives_a_different_audit_failing_before_worker_delivery`, `per_6_saved_project_receipt_frames_are_local_to_the_call_and_never_copied`, `shutdown_report_is_not_silently_discarded`; PGR-1–PGR-5 cover storage; [real CLI trust, restart and revoke journey](../../scripts/smoke-permissions.py) |
| PER-7 | `per_7_native_setting_is_a_named_grant_with_current_revision_controls`, `per_7_permission_controls_review_cancel_submit_and_refresh_by_identity`, `per_7_permission_controls_frames_keep_scope_and_confirmation_visible`, `session_rows_live_in_the_menu_and_project_rows_in_the_drawer`, `per_7_session_setting_before_first_turn_survives_new_and_revokes_without_jsonl`, `per_7_session_setting_releases_native_waiters_but_leaves_commands_pending` |
| PER-8 | `per_8_project_allow_requires_exact_personal_trust_and_edits_invalidate_the_review`, `per_8_untrusted_project_ask_and_deny_precede_user_allow_and_bad_sources_refuse`, `per_8_rule_grammar_is_strict_bounded_and_compiles_complete_sources`, `per_8_trusted_configuration_runs_the_command_and_dispatch_rechecks_changed_bytes`, `per_8_project_rule_review_scrolls_full_scopes_before_separate_confirmation`, `per_8_project_trust_frames_show_source_scopes_and_confirmation`; [real CLI trust, restart and revoke journey](../../scripts/smoke-permissions.py) |
| PER-9 | `per_9_decision_evidence_tracks_precedence_and_exact_source_revisions`, `per_9_historical_command_scopes_preserve_context_and_recheck_wire_bounds`, `per_9_permission_history_replays_without_authority_or_model_content`, `per_9_provenance_refuses_foreign_late_and_duplicate_call_facts`, `per_5_failed_remember_audit_never_dispatches_the_prepared_command`, `per_5_revocation_between_preparation_and_dispatch_refuses_the_effect` |
| PER-10 | `per_10_literal_shell_preserves_posix_quotes_tokens_spans_and_complete_sequences`, `per_10_expansions_redirections_control_flow_and_ambiguous_syntax_have_no_literal_scope`, `per_10_parser_limits_never_publish_partial_literal_commands`, `per_10_generated_literal_words_agree_with_bin_sh`, `per_10_arbitrary_shell_source_stays_bounded`, `per_10_catalog_prefix_matches_whole_literal_calls_and_never_peels_wrappers`, `per_10_prefix_ask_and_deny_cover_any_literal_sibling_before_exact_allow`, `per_10_prefix_wire_roundtrip_preserves_tokens_and_refuses_invalid_scopes`, `per_10_cancelled_prefix_admission_publishes_no_permission_subject`, `per_10_configured_prefix_tokens_compile_with_catalog_context_and_strict_bounds`, `per_10_project_prefix_reuses_changed_arguments_and_external_revoke_stops_dispatch`, `per_10_uncovered_syntax_keeps_exact_review_and_explicit_rules_precede_reuse`, `per_10_offers_use_meaningful_floors_and_explain_exact_fallback`, `per_10_prefix_permission_frames_show_tokens_context_and_project_lifetime` with the `prefix-permission-*` frames, `per_10_short_prefix_confirmation_retains_scope_and_all_choices`, `per_10_keyboard_and_pointer_cannot_confirm_a_scope_clipped_by_the_draft`; [real CLI trust, restart and revoke journey](../../scripts/smoke-permissions.py) |

## Bounds and ownership

The Session holds at most 128 user rules and 128 temporary grants; the current project configuration
has its own 128-rule bound. Command subjects retain at most 24 KiB and native paths at most 4 KiB.
The runtime shares the Session owner explicitly across Conversation replacement. A poisoned owner
fails closed. No mutable process global or journal replay path creates authority.
PGR-1–PGR-5 defines project storage.

## Configuration

Both user `config.toml` under PRV-6's root and project `.plexmaton/config.toml` accept:

```toml
[[permissions.rules]]
action = "allow"
match = { kind = "native_file_changes" }

[[permissions.rules]]
action = "ask"
match = { kind = "exact_command", source = "git fetch origin" }

[[permissions.rules]]
action = "allow"
match = { kind = "command_prefix", arguments = ["git", "fetch"] }
```

Actions are `allow`, `ask`, or `deny`. Match kinds name the native preset, exact shell source or
a literal argv prefix. Unknown fields, more than 128 rules, and commands outside CMD-1 bounds refuse the complete
source. The catalog compiles definitions and command context; configuration cannot supply either.
Project files retain SKL-1's 64 KiB complete-read bound. User rules load once per coding Session;
restart reloads them. Project rules refresh under the personal store lock before controls and dispatch.

The Drawer's Permissions page reviews and revokes Session/Project grants, controls the Session native preset, and
reviews project rules. In project review, Up/Down or the wheel scroll complete escaped scopes;
Enter or the visible Continue action opens activation confirmation. Back is selected initially.
Esc returns one page. Activation applies only to the reviewed SHA-256 fingerprint and permission
revision. Withdrawal remains available if the project file changed or disappeared. Trust and grants
live outside Conversation JSONL; no authority is recovered from a historical decision.

## Literal command grammar

The command worker parses with tree-sitter Bash and lowers a POSIX subset for `/bin/sh -c`:
complete simple literal commands joined by `;`, newline, `&&` or `||`. Quotes, concatenation and
literal escapes preserve argument boundaries, including empty arguments. Variable/command
expansion, globbing, redirects, pipelines, background jobs, control flow and alias manipulation
have no reusable parse. A backslash-newline gap between word nodes is rejected because the parser
can split argv where `/bin/sh` joins it. Quoted data is decoded from its complete source span.

One Allow rule or grant must cover the entire sequence; separate partial grants are not combined.
A restrictive prefix applies to any literal sibling in that complete parse. These matchers do not
infer commands inside unsupported dynamic syntax. Exact permissions still match exact source and
PER-4 context regardless of parser availability. A wrapper is never peeled and a leading option
never discarded: `git -C ../other fetch` cannot match `["git", "fetch"]`.

Suggestions are deliberately small: `ls`, or `git` followed immediately by `fetch`, `status`,
`diff`, `log` or `show`. They apply only to one simple command. Other commands and compounds offer
exact reuse with an explanatory note. Explicit configuration can choose a narrower literal argv
prefix, including a quoted argument such as `["git", "fetch", "team origin"]`; the UI currently
chooses lifetime for the one backend-issued offer, without a prefix editor.

| Boundary | Limit |
| --- | --- |
| Source | CMD-1 bounds, at most 24 KiB |
| Parser | 512 progress callbacks (the pinned engine checks every 100 parser operations), 20 ms deadline checked at callbacks, owner cancellation |
| Tree lowering | 2048 nodes, depth 32, 32 commands, 128 arguments per command, 24 KiB decoded total |
| Persisted prefix | 32 arguments, 4096 decoded bytes, nonempty executable, no NUL |
| Lifetime | Parser and tree are local to one retained admission worker; no parser cache or detached work |

Capacity, parser failure and unsupported syntax produce typed exact fallback. No shell execution,
expansion, PATH lookup or external process derives permission tokens. Differential tests use only a
fixed `printf` and quoted generated data as the `/bin/sh` oracle.

### Dependency admission

Audited 2026-09-06 from pinned registry sources and the local Codex shell adapter comparison.
`tree-sitter` 0.25.10 and `tree-sitter-bash` 0.25.1 are upstream Tree-sitter packages, MIT, with the
matching 0.25 grammar API. The engine declares Rust 1.76; the grammar declares no MSRV and compiles
with the workspace's 1.98 pin. Bundled C builds through the existing `cc`; no system parser library,
Wasm, bindgen or language-runtime dependency is enabled. The engine uses only `std`.
New transitive packages are `tree-sitter-language` 0.1.7 (MIT, Rust 1.77) and `streaming-iterator`
0.1.9 (MIT/Apache-2.0, Rust 1.56). Existing regex, JSON and build dependencies keep their locked
versions. `cargo tree -d`/`-e features` were inspected and the offline cached-advisory `cargo deny`
audit passed. This does not claim a freshly fetched advisory database.
