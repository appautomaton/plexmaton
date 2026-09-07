# Spec — Agent instructions

| Field | Value |
| --- | --- |
| Status | Implemented; live model adherence unverified |
| Owns | AGENTS.md discovery, scope, bounded request snapshot and conversation-open lifecycle |
| Depends on | SKL-1, WFS-1, PRV-4/PRV-6, BUD-1–BUD-4, CPL-2/CPL-3 |
| Proven by | CLI discovery/lifecycle tests and four-dialect provider tests below |

## Invariants

**AGI-1 — Discovery follows one physical project scope.** Load optional `PLEXMATON_HOME/AGENTS.md`
first, then `AGENTS.md` in each directory from SKL-1's project root through physical cwd, in that
order and once per path. Do not traverse siblings, descendants, a linked worktree's common Git
directory or ancestors above its checkout.
Rejected: filesystem-root ancestry, because nested task worktrees would inherit the primary
checkout's duplicate instructions and project configuration already has a physical root owner.

**AGI-2 — Loaded instructions are complete, bounded and attributed.** Files retain exact UTF-8
content and source/scope paths; missing files are optional, while non-files, symlinked instruction
files, invalid UTF-8/NUL, changed reads, cancellation and limit violations refuse the load. Acquisition uses
WFS-1's pinned reader, and publication never truncates a rule into apparent completeness.

**AGI-3 — Repository prose is user-level context, not execution authority.** Every codec emits the
same snapshot once before journal history in its user role, preserving configured system
instructions. The scope preamble gives explicit user requests precedence, deeper applicable files
precedence over ancestors, and directs the model to read nested files before working there;
instruction loading does not widen tools or grant permissions.

**AGI-4 — Instructions participate in the existing environment.** BUD-3 budgets their actual
encoded bytes once; BUD-2 invalidates usage anchors when the snapshot changes. CPL-2 keeps the
snapshot in the summarizer's unchanged prefix, and CPL-3 retains it outside the compacted history.
Debug and ledger diagnostics contain no loaded document text.

**AGI-5 — A conversation uses one snapshot until reopened.** Startup loads before terminal
acquisition; conversation creation/resume through the picker loads in its owned cancellable
worker before replacing the runtime. File edits cannot change a running request or compaction;
reopening reads current files, while journal replay performs no instruction read or mutation.
Rejected: storing another prompt history in JSONL, because PRV-4 already defines deterministic
replay from journal plus current resolved environment; and watchers without a user-facing reload
contract.

## Grammar and bounds

Only the exact filename `AGENTS.md` is recognized. There is no frontmatter parser, compatibility
fallback name, Git-ignore filter or interpretation of Markdown links as automatic file reads.
User-home content applies to the conversation; project content applies beneath its containing
directory. Configured authority roots and cwd are canonicalized; descendant instruction paths use
no-follow reads. Global and project files at the same physical path are loaded once.

The project root is the existing SKL-1 result: nearest valid Git checkout/worktree above cwd, or
cwd without Git. At most 128 project directories are visited. The complete rendered snapshot,
including its scope preamble and JSON source envelopes, is at most 64 KiB. Oversized source files
are refused after a bounded read; JSON escaping and source paths count toward publication too.
Empty/whitespace-only files add no content; retained nonempty bodies are not trimmed.

Nested files below cwd are not eagerly scanned or automatically injected on tool access. The
preamble instructs the model to discover and read them using ordinary tools; those reads retain
JRN-5 tool outcomes. Model adherence to prose is not a permission guarantee or an offline-test
claim. Restarting, `/new`, or switching to a saved Conversation refreshes the initial snapshot;
selecting the already-open Conversation is a no-op. Changing reasoning effort retains the snapshot.
It is request environment, not a visible user turn or journal entry.

## Evidence

| Invariant | Proven by |
| --- | --- |
| AGI-1 | `agi_1_global_then_project_ancestry_preserves_sources_and_exact_text`, `agi_1_no_git_and_overlapping_home_have_one_source`, `agi_1_worktree_and_symlinked_cwd_use_the_physical_checkout` |
| AGI-2 | `agi_2_missing_and_empty_files_need_no_runtime_state`, `agi_2_invalid_text_non_files_and_symlinks_refuse_loading`, `agi_2_exact_publication_bound_and_escaping_are_accounted_for`, `agi_2_cancellation_and_directory_bounds_refuse_without_a_snapshot`, `agi_2_provider_snapshot_bound_is_validated_and_redacted`; WFS-1's `bounded_reads_are_pinned_no_follow_and_cancelled_before_return`, `bounded_read_reports_completion_without_retaining_the_probe_byte` |
| AGI-3 | `agi_3_workspace_instructions_use_user_roles_in_every_dialect`, `agi_1_global_then_project_ancestry_preserves_sources_and_exact_text`, `agi_2_missing_and_empty_files_need_no_runtime_state`, `agi_5_wire_snapshot_is_stable_then_refreshes_on_jsonl_resume`; model adherence to the [scope preamble](../../crates/plexmaton-cli/src/agent_instructions/prompt.md) remains unverified |
| AGI-4 | `agi_4_instruction_bytes_are_budgeted_and_changed_rules_invalidate_measurements`, `agi_4_workspace_instructions_remain_outside_the_compaction_cut`, `cpl_2_compaction_appends_only_the_instruction_across_all_dialects`, `agi_2_provider_snapshot_bound_is_validated_and_redacted` |
| AGI-5 | `agi_5_wire_snapshot_is_stable_then_refreshes_on_jsonl_resume`, `agi_5_failed_or_cancelled_instruction_load_preserves_the_open_runtime`, `agi_5_invalid_instructions_fail_before_terminal_and_session_creation`, `agi_5_workspace_snapshot_replacement_does_not_accumulate_old_rules`, `agi_3_workspace_instructions_use_user_roles_in_every_dialect` |
