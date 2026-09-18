# Evidence — Conversation model selection

What proves [model-selection](../specs/model-selection.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| MDL-1 | `model_replacement_is_atomic_and_preserves_workspace_instructions`, `model_replacement_refuses_busy_and_incompatible_replay_without_mutation`; `status_resume_isolates_context_refusal_without_weakening_model_admission`; shared EFF-1 admission witnesses |
| MDL-2 | `model_menu_filters_exact_pairs_and_retains_refusal_until_acceptance`, `model_menu_empty_and_dismissed_queries_never_submit_messages`, `model_catalog_bounds_are_visible_and_preserve_complete_identities`, `model_refusal_does_not_follow_keyboard_pointer_or_filter_to_another_choice`, `model_click_without_hover_retains_clicked_identity_after_refusal`; `scripts/smoke-tui.py` and `scripts/smoke-model.py` |
| MDL-3 | `model_replacement_is_atomic_and_preserves_workspace_instructions`, `model_credentials_are_removed_before_install_and_fingerprinting`; `scripts/smoke-model.py` verifies both custom credential names are absent in real command and status children, with retained history/guidance at the second endpoint |
| MDL-4 | `model_override_expires_on_new_resume_and_restart`; `scripts/smoke-model.py` proves `/new` restores the configured default without rewriting configuration |
