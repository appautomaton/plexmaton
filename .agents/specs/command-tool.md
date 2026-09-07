# Spec — Foreground command tool

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Strict command admission, foreground Unix process-group ownership, bounded output capture and typed completion |
| Depends on | [tool-admission](./tool-admission.md) APV-1 through APV-3; [agent-loop](./agent-loop.md) LOOP-2 |
| Proven by | `plexmaton-command::{admission,capture,executor}`, `plexmaton-runtime::runtime::tests::tools`, and `plexmaton-tui::frames` tests |

## Invariants

**CMD-1 — Admission fixes the operation.** The provider-neutral strict `exec_command` schema
requires `cmd` and nullable `timeout_ms`, and refuses additional properties. Admission accepts a
required non-empty `cmd` string bounded to 6,144 Unicode scalars and 24 KiB of UTF-8, plus a
missing, null or non-zero `timeout_ms` no greater than the hard ceiling. Those limits leave room
for worst-case JSON escaping and the bounded root inside the 64 KiB raw-call and 65 KiB canonical
ceilings. Admission pins the definition revision, root, timeout and
`[FileRead, FileWrite, ProcessSpawn]`; the executor revalidates them and the root's file identity
(APV-1 through APV-3). Approval detail bounds each root and command with head, tail and exact
omitted-byte count; the command leads so execution is visible before context wraps. Transcript
invocation retains the original canonical command, root and timeout as `ToolDetail::Command` within
admitted-state bounds. Display summaries do not become the source for inspection or copying (APD-1).

**CMD-2 — The process context is explicit and noninteractive.** One `/bin/sh -c` root starts in its
own process group, with null stdin and the admitted root as `cwd`. The tool snapshots the owner's
OS-string environment once, removes every configured provider's exact API-key variable (MDL-3) plus
Plexmaton-private and credential-shaped variables, then uses a cleared child environment to install
that snapshot plus canonical `PWD` and fixed no-colour, noninteractive pager and prompt controls.
Real `PATH`, `HOME`, locale and toolchain configuration survive; `OLDPWD` does not. There is no PTY,
background API or `write_stdin` path.

Rejected: removing `HOME` while approved commands retain filesystem authority; it breaks user
toolchains without creating confinement.

**CMD-3 — Retention cannot stop drainage.** Stdout and stderr have independent owned drains which
keep reading after retention fills. Each retains at most 64 KiB of raw-byte head and tail with the
exact bytes read and omitted-byte count. After root and owned-group completion, EOF gets a bounded
grace; expiry seals both drains cooperatively and returns the partial captures instead of waiting
forever on an escaped pipe holder. A second aggregate byte bound applies after lossy UTF-8
conversion, so invalid input cannot expand the model result past its limit. Only the final typed
result exposes output; transcript detail is the exact model text plus omission metadata.

**CMD-4 — Completion is typed.** A result distinguishes an exit code, a Unix signal, timeout and
cancellation. Text and numeric conventions are presentation, never lifecycle control flow.

**CMD-5 — Stop settles owned-group authority before returning.** Cancellation wins a simultaneously
observable timeout; the hard deadline wins over a simultaneously ready completion. Either stop
sends `SIGTERM` to the process group and returns early if it becomes quiescent; at grace expiry any
remaining group or unreaped root receives `SIGKILL`, then the root is reaped, group disappearance is
bounded, and both drains join. A naturally exited root follows the same sequence for descendants.

**CMD-6 — Cleanup is an explicit awaited transition.** The caller drives execution to completion
and cancels through its token; there is no fire-and-forget task and no detached `Drop` reaper.
The live runtime retains the execution worker across cancelled event polls; interrupt and shutdown
cancel and join it before returning, while direct runtime `Drop` does the same as a resource-safety
backstop. Partial output streaming remains outside this boundary.

Rejected: detached Tokio tasks in place of joined workers, which would make direct `Drop` abandon
cleanup. A later bounded supervisor may pool workers without changing this ownership rule.

## Workspace-root limit

The canonical root is a starting directory, not a sandbox. An approved shell can use absolute
paths, `..`, symlinks, network and inherited host authority. Environment scrubbing is secret
hygiene; files and sockets remain. A descendant may escape group signalling with a new session;
bounded drainage prevents retention but does not terminate it. Either escape needs later OS
containment, and this implementation claims none.

The root's device and inode are rechecked immediately before spawn. POSIX offers no safe,
first-party-`unsafe`-free way to make the final check and `current_dir(path)` one atomic operation,
so a same-user path replacement in that final interval is outside the guarantee. This is a
path-rooted execution boundary with stale-root detection, not descriptor-rooted confinement.

The implementation currently supports Unix only. The fixed shell path and signal numbers are
covered on the host gate; no Windows job-object adapter exists.

## Evidence

| Invariant | Proven by |
| --- | --- |
| CMD-1 | `cmd_1_model_schema_is_strict_nullable_and_exposes_timeout_bounds`, `cmd_1_admission_is_strict_canonical_and_pins_the_workspace`, `cmd_1_refuses_every_shape_outside_the_model_contract_and_hard_bounds`, `cmd_1_multibyte_and_escaped_commands_fit_every_advertised_bound`, `cmd_1_approval_detail_leads_with_command_and_bounds_root_separately`, `native_command_is_visible_before_decision_at_the_smallest_terminal`, `cmd_1_executor_refuses_a_call_pinned_to_another_workspace`, `cmd_1_changed_workspace_identity_is_refused_before_spawn` |
| CMD-2 | `cmd_2_and_cmd_4_use_fixed_noninteractive_context_and_typed_exit`, `cmd_2_snapshot_preserves_path_and_home_but_scrubs_private_authority`, `cmd_2_environment_snapshot_preserves_non_unicode_entries`, `cmd_2_environment_snapshot_scrubs_credential_shaped_names`, `cmd_2_selected_api_key_environment_is_removed_even_without_credential_shape`, `model_credentials_are_removed_before_install_and_fingerprinting` |
| CMD-3 | `cmd_3_capture_keeps_exact_raw_head_tail_and_omission_across_chunking`, `cmd_3_utf8_projection_keeps_a_character_split_between_head_and_tail`, `cmd_3_drains_one_mibibyte_from_each_pipe_after_retention_fills`, `cmd_3_completed_drain_is_not_polled_again_when_the_sibling_is_sealed`, `cmd_3_preserves_invalid_utf8_as_raw_bytes_and_bounds_its_text_view`, `cmd_3_escaped_pipe_holder_is_sealed_and_joined_with_partial_evidence`, `cmd_3_and_cmd_4_model_formatter_is_typed_and_hard_bounded_after_lossy_utf8` |
| CMD-4 | `cmd_2_and_cmd_4_use_fixed_noninteractive_context_and_typed_exit`, `cmd_3_and_cmd_4_model_formatter_is_typed_and_hard_bounded_after_lossy_utf8`, `cmd_5_timeout_has_a_typed_cause_and_leaves_no_process_group`, `cmd_5_cancellation_gracefully_terms_reaps_and_joins_drains` |
| CMD-5 | `cmd_5_cancellation_gracefully_terms_reaps_and_joins_drains`, `cmd_5_timeout_has_a_typed_cause_and_leaves_no_process_group`, `cmd_5_interrupt_wins_when_timeout_is_simultaneously_ready`, `cmd_5_deadline_wins_when_completion_is_already_ready`, `cmd_5_group_disappearing_at_sigkill_does_not_report_it_sent`, `cmd_5_permission_denied_probe_still_reports_an_existing_group`, `cmd_5_root_exit_terminates_a_descendant_holding_an_inherited_pipe`, `cmd_5_signal_failure_retains_primary_error_and_reaps_the_owned_group`, `cmd_5_kill_failure_is_retried_while_retaining_the_primary_error`, `cmd_5_inspect_failure_retains_primary_error_and_reaps_the_owned_group` |
| CMD-6 | `cmd_6_pre_cancelled_command_never_spawns`, `cmd_5_wait_failure_retains_primary_error_and_reaps_the_owned_group`, `cmd_5_try_wait_failure_retains_primary_error_and_reaps_the_owned_group`, `cmd_3_escaped_pipe_holder_is_sealed_and_joined_with_partial_evidence`, `cancelled_next_event_keeps_command_work_owned_until_interrupt_joins_it`, `cancelled_shutdown_can_be_called_again_to_finish_exact_cleanup`, `dropping_an_active_runtime_joins_its_command_worker_and_process_group` |
