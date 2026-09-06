# Spec — Agent Skills

| Field | Value |
| --- | --- |
| Status | Implemented; live model behavior unverified |
| Owns | Project settings, skill discovery, scoped reads and explicit activation |
| Depends on | PRV-6, WFS-1, APV-1/APV-2, JRN-5/JRN-7, BUD-2 |
| Proven by | Filesystem, agent, provider, runtime and CLI tests in the evidence table |

## Invariants

**SKL-1 — Project settings do not relocate user state.** The physical checkout/worktree's optional
`.plexmaton/config.toml` selects an exact user-defined provider/model pair; unknown fields and invalid
present files fail explicitly. PER-8 owns its permission rules and separate personal trust.
`PLEXMATON_HOME` retains configuration and runtime ownership (PRV-6).

**SKL-2 — Discovery publishes a bounded deterministic catalog.** Direct bundles are discovered in
project `.plexmaton/skills`, project `.agents/skills`, then user-home `skills`; first validated name
wins with a shadowing diagnostic. Metadata is validated before it enters bounded model tool definitions.

**SKL-3 — A skill reader owns one filesystem scope.** Bodies and relative resources resolve beneath
the pinned winning source without traversal or symlink authority; a missing or changed winner never
falls back to another origin. Reads have byte bounds, cancellation and explicit failure (WFS-1).

**SKL-4 — Invocation policy is separate from execution authority.** `disable-model-invocation` and
`user-invocable` independently filter and gate activation; metadata never grants capabilities.
Model skill reads enter normal admission as `FileRead`; scripts retain ordinary process policy (APV-2).

**SKL-5 — Activated content is a durable semantic fact.** Model activations retain exact content in
normal tool outcomes; explicit invocations retain the original user text and a separately typed skill
activation before dependent model dispatch. Replay reads only those records, and queue bounds include
attached content (JRN-5/JRN-7). Ordinary retry preserves the prior body; explicit edit/retry loads a
new body before replacing its branch, returning the exact edit on failure.

**SKL-6 — Skills use the existing runtime and request accounting.** Metadata is part of the skill
tool definition and therefore the request environment fingerprint; explicit content is a context atom
budgeted by every codec. All filesystem work is off the TUI event loop with owned cancellation and
completion; current catalog changes cannot reuse a mismatched measurement (BUD-2).

## Grammar and bounds

User configuration remains `PLEXMATON_HOME/config.toml`. Project configuration supports optional
`[active_model]` with `provider` and `model`, plus PER-8 permission rules; absence preserves user selection. Find the nearest valid
Git root above cwd, accepting linked-worktree gitfiles; without Git use cwd. Project discovery never
widens native workspace file tools. The project configuration limit is 64 KiB.

Skills are `<root>/<name>/SKILL.md` with YAML frontmatter and Markdown instructions. Follow the
[format](https://agentskills.io/specification) and the reference validator's NFKC Unicode names;
client invocation controls are optional extensions, both enabled by default. Unknown metadata adds
no behavior. Cap discovery at 256 candidates, frontmatter at 16 KiB, model catalog at 64 KiB and
loaded body/resource at 256 KiB. Over-limit cases are diagnostics or typed refusals, never truncation
that looks complete. Catalogs retain summaries, not bodies.

The model calls `skill` with `name` and optional `resource`; omission loads `SKILL.md`, and a resource
is relative to the selected bundle. The [composer picker](./skill-picker.md) owns `$name` invocation
and selected-name metadata; only known user-invocable names are inferred from plain dollar text.
Preserve that full user text; load instructions independently
and retain source/name/canonical location/digest with the exact content. A model may read resources
from a user-only skill only after that exact source was explicitly activated on the selected journal
path. Resource reads do not execute scripts.

Explicit edit/retry transfers the edit to owned preparation before changing a branch. A failed
load returns that text once; a successful branch replacement preserves any new composer input.
Returned input retains the selected name, so a deliberately selected numeric skill is not confused
with literal currency when a load, queue handoff or durable write fails.

The catalog is current request environment, like other tool definitions; historical tool outcomes
and explicit skill atoms are journal facts. On resume, files may change the current catalog but never
rewrite recorded instruction text. CPL-3 retains the current user/skill pair exactly during automatic
compaction; older activations may enter its digest, with their source records preserved.

## Evidence

| Invariant | Proven by |
| --- | --- |
| SKL-1 | `project_selection_overrides_only_the_user_active_model`, `project_config_rejects_authority_and_unrecognized_fields`, `project_config_refuses_symlinks_at_every_component`, `linked_worktree_uses_its_checkout_instead_of_the_common_repo` |
| SKL-2 | `collisions_select_one_origin_and_missing_winners_do_not_fall_back`, `metadata_is_strict_typed_and_unicode_normalized`, `candidate_frontmatter_catalog_and_content_bounds_are_typed`, `model_skill_call_records_real_read_while_catalog_omits_body` |
| SKL-3 | `bounded_reads_are_pinned_no_follow_and_cancelled_before_return`, `bounded_read_reports_completion_without_retaining_the_probe_byte`, `resources_are_exact_confined_and_recheck_invocation_after_replacement`, `derived_skill_root_symlinks_never_load_external_metadata`, `directory_listing_is_bounded_and_cancellable`, `cancellation_precedes_discovery_and_reads` |
| SKL-4 | `explicit_user_only_skill_allows_resource_but_not_unprompted_body`, `unavailable_explicit_skills_restore_input_without_model_dispatch` |
| SKL-5 | `cpl_3_skill_invocation_survives_compaction_in_every_dialect`, `skl_5_explicit_submission_records_skill_separately_before_dispatch`, `skl_5_queued_turn_and_steering_retain_skill_activation`, `skl_5_skill_activation_rejects_wrong_ownership_and_order_without_mutation`, `explicit_skill_context_survives_source_deletion_and_jsonl_resume`, `edited_retry_prepares_skill_before_replacing_the_failed_branch` |
| SKL-6 | `skill_preparation_completes_while_compaction_is_waiting`, `skl_6_every_codec_budgets_exact_skill_wire_content`, `skill_context_is_exact_and_uses_supported_user_roles_in_every_dialect`, `skill_preparation_cannot_starve_interrupt_at_input_capacity`, `skill_preparation_preserves_shutdown_and_persistence_failure_causes`, `the_skill_diagnostic_frames_match_their_fixtures` |
