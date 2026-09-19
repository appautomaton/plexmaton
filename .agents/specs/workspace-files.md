# Spec — Workspace file tools

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Workspace path authority, bounded exact text reads, observed file versions and windows, and bounded ripgrep search |
| Depends on | [tool-admission](./tool-admission.md) APV-1 through APV-3 |
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

[Named proofs](../evidence/workspace-files.md), one row an invariant.
