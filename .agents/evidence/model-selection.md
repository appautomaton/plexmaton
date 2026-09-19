# Evidence — Conversation model selection

What proves [model-selection](../specs/model-selection.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| MDL-1 | `model_replacement_is_atomic_and_preserves_workspace_instructions`, `model_replacement_refuses_busy_and_degrades_foreign_replay_without_mutation`, `model_replacement_between_two_models_of_one_provider_is_not_refused`, `the_degraded_history_frames_match_their_fixtures` with the `degraded-history-*` frames; `status_resume_reads_another_dialects_history_without_losing_observed_facts`; shared EFF-1 admission witnesses |
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
