# Spec — Conversation model selection

| Field | Value |
| --- | --- |
| Status | Implemented; local runtime, menu and executable witnesses passed |
| Owns | Exact configured model selection, idle replacement, and conversation-local model overrides |
| Depends on | CMC-1/CMC-2, SKP-3/SKP-4, PRV-6, CMD-2, EFF-1/EFF-5, STL-3 |
| Proven by | Runtime and TUI tests below, plus the offline model PTY journey |

## Invariants

**MDL-1 — Replacement has one idle boundary.** Model and effort changes share runtime admission:
wrong-agent, busy/queued work, approval, compaction, persistence failure and shutdown refuse a
replacement. A fully constructed driver replaces the current one atomically, without starting a
request or altering canonical history; failure leaves the old driver intact. The selected journal
projection must encode under the destination model before acceptance; incompatible provider replay
is refused without stripping its sidecars.

**MDL-2 — A model is an exact configured pair.** The CLI supplies bounded menu summaries from its
immutable provider/model registry; a selected row carries both identities. Only acceptance updates
the confirmed model, effort capabilities, context budget and status. Filtering, hover and Escape
never apply a choice; unknown queries never become a model request.

**MDL-3 — Switching cannot expose credentials or reuse the wrong request environment.** Every
switchable provider credential name is excluded from captured command environments before scopes
are compiled. Replacement refuses an unprotected credential, retains the workspace instruction
snapshot and native tool owners, and computes the destination model's request environment.

**MDL-4 — Overrides belong to the open conversation.** A selected model uses its configured effort
default and persists while that conversation remains open. New, resume and restart use the
configured default. Model selection writes neither user/project configuration nor historical
request metadata; EFF-5 owns the corresponding effort lifetime. Opening saved history does not
assert that the default model can encode it; STL-3 keeps historical status available when its
prospective context is unavailable.

## Grammar

`/model` opens the existing composer menu. `/model <query>` filters configured model names,
display names, wire IDs and provider names (case-insensitive substring matching). Arrows or actual
pointer movement choose; Enter confirms and
Escape dismisses while retaining the draft; Tab never confirms a model row. A matching press/release
also confirms the row by identity (INV-11). A failed selection keeps the menu and prior model;
choosing another row clears the old refusal. Refusal text uses the theme's Failure style, including
wrapped lines; catalog-limit and empty-list explanations retain the Muted style.
No configuration discovery or network request is made by opening the menu. The catalog retains at
most 256 complete entries / 64 KiB of metadata and identifies a limited list explicitly. Empty and
no-match lists remain open; no query becomes a provider prompt. Rows display provider/configured
name, display name and wire ID, with the exact accepted pair marked current.

## Evidence

| Invariant | Proven by |
| --- | --- |
| MDL-1 | `model_replacement_is_atomic_and_preserves_workspace_instructions`, `model_replacement_refuses_busy_and_incompatible_replay_without_mutation`; `status_resume_isolates_context_refusal_without_weakening_model_admission`; shared EFF-1 admission witnesses |
| MDL-2 | `model_menu_filters_exact_pairs_and_retains_refusal_until_acceptance`, `model_menu_empty_and_dismissed_queries_never_submit_messages`, `model_catalog_bounds_are_visible_and_preserve_complete_identities`, `model_refusal_does_not_follow_keyboard_pointer_or_filter_to_another_choice`, `model_click_without_hover_retains_clicked_identity_after_refusal`; `scripts/smoke-tui.py` and `scripts/smoke-model.py` |
| MDL-3 | `model_replacement_is_atomic_and_preserves_workspace_instructions`, `model_credentials_are_removed_before_install_and_fingerprinting`; `scripts/smoke-model.py` verifies both custom credential names are absent in real command and status children, with retained history/guidance at the second endpoint |
| MDL-4 | `model_override_expires_on_new_resume_and_restart`; `scripts/smoke-model.py` proves `/new` restores the configured default without rewriting configuration |

## Rendered review

Actual `model_preview` cell-buffer frames, 120 / 88 / 60 columns:
[choices wide](../../crates/plexmaton-tui/frames/models/models-120.svg),
[medium](../../crates/plexmaton-tui/frames/models/models-88.svg),
[narrow](../../crates/plexmaton-tui/frames/models/models-60.svg);
[refusal wide](../../crates/plexmaton-tui/frames/models/refusal-120.svg),
[medium](../../crates/plexmaton-tui/frames/models/refusal-88.svg),
[narrow](../../crates/plexmaton-tui/frames/models/refusal-60.svg).

The PTY journey uses two owned Chat Completions fixture endpoints, exact wire model/effort
assertions, missing-credential refusal, canonical history and workspace guidance, real command
approval, status output, and
new-conversation reset. It never contacts a configured live provider or mutates live configuration.
Cross-dialect replacement and incompatible Responses replay are covered at the runtime boundary.
