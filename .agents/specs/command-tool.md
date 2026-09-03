# Spec — Foreground command tool

| Field | Value |
| --- | --- |
| Status | Implemented for Phase 01 stage 2 slice 9 |
| Owns | Strict command admission, foreground Unix process-group ownership, bounded output capture and typed completion |
| Depends on | [tool-admission](./tool-admission.md) APV-1 through APV-3; [agent-loop](./agent-loop.md) LOOP-2; Phase 01 §producer |
| Proven by | `plexmaton-command::{admission,capture,executor}` tests |

## Invariants

**CMD-1 — Admission fixes the operation.** The provider-neutral strict `exec_command` schema
requires `cmd` and nullable `timeout_ms`, and refuses additional properties. Admission accepts a
required non-empty `cmd` string bounded to 6,144 Unicode scalars and 24 KiB of UTF-8, plus a
missing, null or non-zero `timeout_ms` no greater than the hard ceiling. Those limits leave room
for worst-case JSON escaping and the separately bounded root inside the 64 KiB admitted-call
ceiling. Admission pins the definition revision, canonical workspace root, normalized timeout and
the canonical capability set `[FileRead, FileWrite, ProcessSpawn]`; the executor revalidates all of
them. It also refuses execution if the root's file identity changed since the tool was constructed
(APV-1 through APV-3). Approval detail independently bounds the root and command, retaining the
head, tail and exact omitted-byte count of either.

**CMD-2 — The process context is explicit and noninteractive.** One `/bin/sh -c` root starts in its
own process group, with null stdin and the admitted root as `cwd`. The tool snapshots the owner's
OS-string environment once, removes Plexmaton-private and credential-shaped variables, then uses a
cleared child environment to install that snapshot plus canonical `PWD` and fixed no-colour,
noninteractive pager and prompt controls. Real `PATH`, `HOME`, locale and toolchain configuration
survive; `OLDPWD` does not. There is no PTY, background API or `write_stdin` path.

**CMD-3 — Retention cannot stop drainage.** Stdout and stderr have independent owned drains which
keep reading after retention fills. Each retains at most 64 KiB of raw-byte head and tail with the
exact bytes read and omitted-byte count. After root and owned-group completion, EOF gets a bounded
grace; expiry seals both drains cooperatively and returns the partial captures instead of waiting
forever on an escaped pipe holder. A second aggregate byte bound applies after lossy UTF-8
conversion, so invalid input cannot expand the model result past its limit. Output becomes
observable only in that final typed result.

**CMD-4 — Completion is typed.** A result distinguishes an exit code, a Unix signal, timeout and
cancellation. Text and numeric conventions are presentation, never lifecycle control flow.

**CMD-5 — Stop settles owned-group authority before returning.** Cancellation wins a simultaneously
observable timeout. Either stop sends `SIGTERM` to the process group and returns early if it becomes
quiescent; at the grace deadline it sends `SIGKILL`, reaps the root, observes group disappearance
within a bounded interval and joins both drains. A naturally exited root follows the same sequence
for remaining members of its process group.

**CMD-6 — Cleanup is an explicit awaited transition.** The caller drives execution to completion
and cancels through its token; there is no fire-and-forget task and no detached `Drop` reaper.
Slice 9 exposes neither live-runtime registration nor partial output streaming.

## Workspace-root limit

The canonical root is an execution starting directory, not a filesystem sandbox. Once policy
allows the broad capability set, the shell can use absolute paths, `..`, symlinks, the network and
any host authority inherited from the Plexmaton process. Environment scrubbing is secret hygiene,
not confinement: the command may still discover authority through files, sockets or other host
interfaces. A descendant that deliberately creates a new session or process group can also escape
Unix process-group signalling; bounded drainage prevents it from retaining this executor, but does
not terminate it. Preventing either escape requires a later OS sandbox/process-containment
mechanism; this implementation does not claim one or silently rewrite the command.

The root's device and inode are rechecked immediately before spawn. POSIX offers no safe,
first-party-`unsafe`-free way to make the final check and `current_dir(path)` one atomic operation,
so a same-user path replacement in that final interval is outside the guarantee. This is a
path-rooted execution boundary with stale-root detection, not descriptor-rooted confinement.

The implementation currently supports Unix only. The fixed shell path and signal numbers are
covered on the host gate; no Windows job-object adapter exists.

## Evidence

| Invariant | Proven by |
| --- | --- |
| CMD-1 | `cmd_1_model_schema_is_strict_nullable_and_exposes_timeout_bounds`, `cmd_1_admission_is_strict_canonical_and_pins_the_workspace`, `cmd_1_refuses_every_shape_outside_the_model_contract_and_hard_bounds`, `cmd_1_multibyte_and_escaped_commands_fit_every_advertised_bound`, `cmd_1_approval_detail_keeps_root_and_command_head_tail_separately`, `cmd_1_executor_refuses_a_call_pinned_to_another_workspace`, `cmd_1_changed_workspace_identity_is_refused_before_spawn` |
| CMD-2 | `cmd_2_and_cmd_4_use_fixed_noninteractive_context_and_typed_exit`, `cmd_2_snapshot_preserves_path_and_home_but_scrubs_private_authority`, `cmd_2_environment_snapshot_preserves_non_unicode_entries`, `cmd_2_environment_snapshot_scrubs_credential_shaped_names` |
| CMD-3 | `cmd_3_capture_keeps_exact_raw_head_tail_and_omission_across_chunking`, `cmd_3_utf8_projection_keeps_a_character_split_between_head_and_tail`, `cmd_3_drains_one_mibibyte_from_each_pipe_after_retention_fills`, `cmd_3_preserves_invalid_utf8_as_raw_bytes_and_bounds_its_text_view`, `cmd_3_escaped_pipe_holder_is_sealed_and_joined_with_partial_evidence`, `cmd_3_and_cmd_4_model_formatter_is_typed_and_hard_bounded_after_lossy_utf8` |
| CMD-4 | `cmd_2_and_cmd_4_use_fixed_noninteractive_context_and_typed_exit`, `cmd_3_and_cmd_4_model_formatter_is_typed_and_hard_bounded_after_lossy_utf8`, `cmd_5_timeout_has_a_typed_cause_and_leaves_no_process_group`, `cmd_5_cancellation_gracefully_terms_reaps_and_joins_drains` |
| CMD-5 | `cmd_5_cancellation_gracefully_terms_reaps_and_joins_drains`, `cmd_5_timeout_has_a_typed_cause_and_leaves_no_process_group`, `cmd_5_interrupt_wins_when_timeout_is_simultaneously_ready`, `cmd_5_root_exit_terminates_a_descendant_holding_an_inherited_pipe`, `cmd_5_signal_failure_retains_primary_error_and_reaps_the_owned_group`, `cmd_5_kill_failure_is_retried_while_retaining_the_primary_error`, `cmd_5_inspect_failure_retains_primary_error_and_reaps_the_owned_group` |
| CMD-6 | `cmd_6_pre_cancelled_command_never_spawns`, `cmd_5_wait_failure_retains_primary_error_and_reaps_the_owned_group`, `cmd_5_try_wait_failure_retains_primary_error_and_reaps_the_owned_group`, `cmd_3_escaped_pipe_holder_is_sealed_and_joined_with_partial_evidence`; live-runtime registration remains Slice 10 |
