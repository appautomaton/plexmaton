# Spec — Workspace mutation

| Field | Value |
| --- | --- |
| Status | Proven through Phase 01 stage 3 slice 2 |
| Owns | Observation-bound exact edits, create-if-absent, bounded staging, and same-directory publication |
| Depends on | [workspace-files](./workspace-files.md) WFS-1 through WFS-3; [tool-admission](./tool-admission.md) APV-1 through APV-3 |
| Proven by | `plexmaton-file-tools::{catalog,mutation,path}` component and integration tests named below |

## Invariants

**MUT-1 — Existing-file intent compiles before policy.** `edit_file` accepts a bounded batch of
exact `old_text`/`new_text` pairs plus one read observation. Admission resolves each unique source
span inside that observation's returned byte window and freezes sorted byte splices against one
original version; the executor never performs fuzzy matching or reinterprets the model's raw edit
syntax.

**MUT-2 — Mutation authority is narrower than its path string.** Every parent is opened beneath the
pinned workspace root with no-follow descriptor semantics, the leaf stays relative to that pinned
parent, and an edit's normalized path must equal its observation's path. Edit declares `FileRead`
and `FileWrite`; the separate absence-only `create_file` declares `FileWrite`.

**MUT-3 — Approval cannot refresh a precondition.** Execution resolves the observation again,
reopens the target without following a leaf symlink, and requires the observed descriptor version,
source length, and admitted source bytes before staging and again immediately before publication.
A change observed by either check is retained and returned as a typed stale result. POSIX exposes
no portable conditional rename: a same-user process racing either the target or staging entry after
its final validation and before the publication syscall is outside this proven guarantee, so this
mechanism is not described as compare-and-swap or an adversarial filesystem sandbox.

**MUT-4 — One edit batch has one visibility point.** Every splice validates against the same
original bytes, overlaps are refused, untouched bytes and ordinary Unix permission bits (`0777`)
are preserved, and the complete result is written and synced in the target directory before one
descriptor-relative rename. Staging contents, descriptor identity, and path identity are rechecked
before the final target validation; cancellation is sampled again at the publication edge. A write
failure or a staging change observed by those checks leaves the original unchanged and removes only
the owned entry. Atomic describes visibility of the completed rename, not MUT-3's weaker concurrency
window.

**MUT-5 — Creation never replaces.** `create_file` requires an existing pinned parent and an absent
leaf at admission, stages bounded UTF-8 content in that directory, validates its exact content and
identity, then uses a no-replace hard link as its publication point. A target creator arriving at
any time wins unchanged; edit failure never falls back to creation or whole-file overwrite.

**MUT-6 — Work and retained state are bounded and cancellable.** One mutation has at most 16 edits,
48 KiB of decoded aggregate UTF-8 model content, an 8 MiB source and result, one staging file, and
8 KiB write chunks. Schema `maxLength` uses its character-count meaning and field descriptions name
the decoded mutation byte guard. Canonical splice structure has a 1 KiB reserve over the 64 KiB
raw-call ceiling. A successful edit's complete canonical patch contains only the path, bounded
changed text and per-edit framing; its hard bound is derived from those limits and never from the
8 MiB source ceiling. Invalid UTF-8/NUL source, NUL creation, over-bound input, cancellation, and
injected write failure publish neither a partial target nor unbounded diagnostics.

## Selected model surface

The Slice 8 trial used the local OpenAI-compatible Responses endpoint with `gpt-5.6-luna`,
`store: false`, low reasoning, one forced strict tool call, and no parallel requests. Five edit
fixtures ran in three rotated repetitions per candidate. Exact replacements, strict patch, and
observed ranges each achieved 15/15 first-attempt calls with no retries; exact replacements retained
2,353 argument bytes and 1,157 output tokens, versus 2,708/1,453 for patch and 2,802/1,299 for
ranges. Exact used 8,136 input tokens versus patch's 7,731 and ranges' 9,303, but won on output,
argument size, and parser-free deterministic compilation. Create-only achieved 6/6 first attempts.

Encrypted reasoning items were replayed exactly when the trial needed another request; only counts
and aggregate usage were retained in the report, never encrypted contents. The winning public
surface is `edit_file { path, observation, edits[] }` plus `create_file { path, content }`; all
provider dialects translate that one semantic catalog rather than owning mutation behavior.

## Failure modes

| Situation | Response |
| --- | --- |
| Unknown, evicted, wrong-path, or changed observation | Typed stale refusal/result; no approval can refresh it |
| Exact text absent outside or inside the window | Typed source mismatch; unobserved text is never writable |
| More than one in-window match | Typed ambiguity; the model must provide more exact context |
| Two source spans overlap | Typed conflict; none of the batch is staged |
| Parent or leaf is a symlink | Typed path failure/collision without following it |
| Existing create leaf or concurrent creator | Typed collision; existing bytes win |
| Cancellation, staging write failure, or pre-publish change | Owned staging entry removed; original or absence preserved |

## Evidence

| Invariant | Proven by |
| --- | --- |
| MUT-1 | `exact_batch_preserves_byte_shape_and_mode`, `admission_enforces_the_observed_window_and_unique_target` |
| MUT-2 | `mutation_paths_pin_the_parent_and_keep_the_leaf_separate`, `mutation_paths_refuse_symlinked_parents_and_leaves`, `execution_rechecks_revision_and_capabilities` |
| MUT-3 | `stale_edit_preserves_the_concurrent_writer`, `writer_before_replace_publication_is_preserved`, `symlink_swap_before_replace_is_refused` |
| MUT-4 | `exact_batch_preserves_byte_shape_and_mode`, `replacement_faults_leave_no_partial_target_or_staging_file`, `replaced_or_modified_staging_entry_is_never_published_or_wrongly_deleted`, `malformed_canonical_and_cancelled_mutations_fail_closed` |
| MUT-5 | `create_is_absence_only_and_never_overwrites`, `create_faults_and_collision_never_publish_staging_bytes`, `mutation_paths_allow_a_missing_leaf_but_not_a_missing_parent` |
| MUT-6 | `mutation_bounds_hold_at_their_exact_edges`, `escaped_edit_within_raw_and_mutation_bounds_survives_canonicalization`, `maximum_edit_canonical_structure_fits_the_one_kibibyte_reserve`, `maximum_valid_edit_retains_a_complete_bounded_patch`, `admission_never_returns_a_trusted_call_after_final_cancellation`, `catalog_never_publishes_a_trusted_call_after_final_cancellation`, `malformed_canonical_and_cancelled_mutations_fail_closed`, `replacement_faults_leave_no_partial_target_or_staging_file`, `create_faults_and_collision_never_publish_staging_bytes` |
