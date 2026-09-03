# Spec — Workspace file tools

| Field | Value |
| --- | --- |
| Status | Proven through Phase 01 stage 3 slice 2 |
| Owns | Workspace path authority, bounded exact text reads, observed file versions and windows, and bounded ripgrep search |
| Depends on | [tool-admission](./tool-admission.md) APV-1 through APV-3; Phase 01 §producer |
| Proven by | `plexmaton-file-tools` component tests and the live-runtime integration tests named below |

## Invariants

**WFS-1 — Paths resolve beneath one pinned workspace root.** Model paths are relative syntax, not
authority: absolute paths, `..`, missing components and every symbolic link are refused. Reads open
each component descriptor-relatively with no-follow semantics. Directory search maps its pinned
descriptor into a minimal driver which `fchdir`s and immediately `exec`s ripgrep only to discover
candidate names. Those names confer no authority: every candidate is reopened descriptor-relatively
with no-follow semantics, and only the resulting pinned regular file is supplied to a separate
ripgrep process over stdin. Each file first becomes a version-checked, bounded in-memory snapshot,
so growth cannot make ripgrep consume beyond the per-file acquisition cap. A single-file search
takes the same pinned-file path. The secure boundary is Unix-only. Rejected: check-then-open
canonical paths, because `rg --no-follow` still follows a symlink supplied as its explicit target.

**WFS-2 — A read is an exact bounded UTF-8 window.** The reader preserves every retained byte,
including BOM, line endings, tabs and final-newline state, while enforcing line-count, line-byte,
result-byte and prefix-scan bounds before allocation. Invalid UTF-8, NUL input, a giant line and a
window beyond the scan budget are typed outcomes; no exact total requires an EOF scan.

**WFS-3 — Successful reads issue bounded authoritative observations.** An opaque session-local ID
names the normalized path, descriptor metadata, and exact returned byte range observed before and
after the read; skipped prefixes and byte-limit lookahead are excluded. A concurrent change refuses
the result, the registry has a hard entry bound, and mutation resolves the ID rather than trusting a
model-supplied hash.

**WFS-4 — Search is argv-based and acquisition-bounded.** Trusted executables must be absolute.
The directory driver may have one fixed trusted argv prefix before the ripgrep executable; model
arguments cannot add to or reorder that prefix. Ripgrep runs with configuration and symlink
following disabled; pattern and glob are distinct arguments while path selects the pinned
descriptor rather than entering a shell. Both search subprocess paths start from an empty
environment with only deterministic presentation variables, so host credentials do not enter the
children. Candidate count,
per-file bytes, match count, record bytes, aggregate transport bytes, stderr, elapsed time and
retained output are independently bounded; reaching a bound kills and joins the exact child instead
of continuing to scan invisibly. Search has no offset pagination which would rescan already skipped
bytes; a bounded result, including a per-file byte limit, tells the model to refine its pattern,
path, or glob. Candidate filtering never bypasses ripgrep's regular-expression validation.

**WFS-5 — Admission parses strict schemas into immutable capability facts.** Unknown fields,
invalid windows and oversized strings are refused before execution. Provider schemas require every
declared property and express optional defaults as nullable values; admission freezes null or
omitted defaults into one canonical form. Schema `maxLength` values are character ceilings while
field descriptions state the decoded UTF-8 byte guards that admission enforces. Relative read and
search paths are lexically normalized once for canonical arguments, presentation, and executor
results. Read and search declare only `FileRead`, and the executor dispatches by pinned definition
identity and revision.
Their canonical path, window or query facts become bounded transcript invocations; successful and
failed results retain a bounded text outcome without changing the exact model-facing result.

**WFS-6 — Cancellation has one owner and observable quiescence.** A cancelled search kills and
joins ripgrep and its readers before returning a typed cancellation. Exceptional process and reader
failures also settle every owned child and I/O task before selecting the returned error. A read
performs only bounded synchronous work and publishes no observation after cancellation is observed.

## Failure modes

| Situation | Response |
| --- | --- |
| Absolute, parent-relative or symlinked path | Typed refusal before file bytes or search output enter state |
| Invalid UTF-8, NUL or oversized line | Typed read failure; no lossy or truncated content and no observation |
| Read reaches a local byte or line cap | Exact retained prefix plus typed completion and next line |
| Search reaches a file, match or transport cap | Exact retained matches plus typed completion; child and I/O tasks joined |
| File changes during read | Typed stale read; no observation is issued |
| Ripgrep is missing or exits abnormally | Typed process failure with bounded stderr |

## Evidence

| Invariant | Proven by |
| --- | --- |
| WFS-1 | `absolute_parent_and_symlink_paths_never_open`, `replacing_the_root_path_does_not_redirect_a_read`, `a_symlinked_search_target_is_refused_before_spawn`, `an_explicit_file_target_swap_never_returns_outside_bytes`, `a_discovered_descendant_is_reopened_no_follow_before_content_search`, `replacing_the_root_path_does_not_redirect_search`, `executable_private_driver_reenters_a_pinned_directory_and_execs_ripgrep` |
| WFS-2 | `read_windows_preserve_exact_bytes_without_scanning_the_tail`, `invalid_binary_and_giant_lines_are_typed` |
| WFS-3 | `successful_reads_issue_bounded_observations`, `a_change_before_the_version_check_refuses_the_read`, `the_observed_range_excludes_skipped_and_lookahead_bytes`, `an_observation_authorizes_only_ranges_inside_its_read_window`, `file_observation_survives_the_runtime_boundary_into_an_approved_edit` |
| WFS-4 | `directory_driver_prefix_precedes_rg_and_model_arguments_exactly`, `search_children_do_not_receive_host_credentials`, `pattern_and_glob_are_distinct_argv_while_path_is_pinned`, `relative_search_executables_are_refused_before_spawn`, `candidate_count_is_bounded_before_unbounded_content_work_starts`, `transport_bytes_are_one_limit_across_discovery_and_every_file`, `a_growing_snapshot_reads_only_one_byte_past_its_hard_bound`, `an_oversized_text_file_reports_its_file_byte_limit`, `an_invalid_pattern_is_rejected_when_every_candidate_is_binary`, `an_invalid_pattern_fails_before_discovery_can_exhaust_transport`, `a_file_change_during_content_search_discards_the_result`, `match_overflow_stops_ripgrep_without_an_unusable_continuation`, `ignored_and_binary_files_do_not_enter_results`, `a_giant_rg_record_is_typed_and_never_retained`, `retained_match_bytes_stop_independently_of_transport`, `stdout_eof_does_not_disable_the_process_deadline` |
| WFS-5 | `catalog_definitions_are_strict_and_bounded`, `model_paths_enforce_the_decoded_utf8_byte_bound`, `search_text_enforces_decoded_utf8_byte_bounds`, `admission_is_strict_and_canonical`, `read_and_search_paths_agree_across_canonical_invocation_and_execution`, `execution_dispatches_by_admitted_definition`, `execution_rechecks_revision_and_capabilities`, `search_execution_accepts_its_own_canonical_arguments`, `file_observation_survives_the_runtime_boundary_into_an_approved_edit` |
| WFS-6 | `a_cancelled_read_publishes_no_observation`, `cancellation_after_read_work_still_publishes_no_observation`, `a_cancelled_file_boundary_precedes_an_oversized_completion`, `a_cancelled_search_joins_its_exact_child_and_reader_tasks`, `a_cancelled_search_returns_only_after_reader_join`, `a_reader_panic_still_joins_every_sibling_before_returning` |
