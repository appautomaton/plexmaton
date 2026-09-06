# Spec — Composer skill picker

| Field | Value |
| --- | --- |
| Status | Implemented; verified offline and at three rendered widths |
| Owns | Primary-composer skill discovery, completion and selection binding |
| Depends on | SKL-2/SKL-4/SKL-5, COM-1/COM-3/COM-6, INV-1/INV-6, SURF-3 |
| Proven by | TUI, runtime and agent proofs below; real terminal completion smoke |

## Invariants

**SKP-1 — Discovery is a projection.** The primary composer consumes a bounded user-invocable
catalog supplied by the composition root; its picker performs no filesystem or runtime operation.
Names, descriptions and origin labels refer to the runtime's winning skill definitions (SKL-2).

**SKP-2 — Completion is an edit with identity.** Choosing a skill inserts `$name ` without sending
the message and binds only that exact initial token. Input handoff and failure retain the selected
name alongside original text; changing the token invalidates its binding (COM-3/COM-6).

**SKP-3 — The composer keeps input ownership.** The central interaction layer routes picker
navigation, acceptance, pointer selection and scrolling; ordinary typing retains the composer's
caret. Escape closes the picker without deleting the draft; worker inputs do not offer this menu.

**SKP-4 — The picker fits the conversation.** The menu is a titled rule and its rows above the
primary composer's top rule, bounded within its conversation column and available terminal
space. It clips summaries and scrolls choices without obscuring the input or taking another
conversation's space.

## Grammar

At the start of a primary draft, `$` opens available skills; subsequent name characters filter.
Up/Down select, Tab or Enter completes, and Escape dismisses. A completed `$name request` submits
normally on the next Enter. Exact unselected nonnumeric names activate only when present in the
user-invocable catalog. Unknown variables, `$HOME`, currency, command substitutions and dollar
expressions inside prose/code remain literal text. Numeric skill names can be deliberately selected
from the menu; unbound `$100` remains currency. There is no `/skill:` execution alias.

The menu shows up to five choices and keeps the selected row and controls visible when height is
constrained; it is suppressed if even one choice and the controls cannot fit. Source labels precede
truncatable descriptions. Displayed metadata is flattened to inert single-line text while its
semantic source remains unchanged. Editing within the initial token can complete without changing
the request suffix. Dismissal survives caret motion until that token changes.

Selected names accompany submitted, returned and retry-editor input. A historical numeric skill
selection comes from its typed journal activation, never from interpreting currency-shaped text.

## Rendered review

The real workspace buffer was inspected at
[wide](../spikes/agent-skills/frames/skill-picker-wide.svg),
[medium](../spikes/agent-skills/frames/skill-picker-medium.svg) and
[narrow](../spikes/agent-skills/frames/skill-picker-narrow.svg) widths. Reproduce with
`cargo run -p plexmaton-tui --example skill_picker_preview -- target/skill-picker-preview`.

## Evidence

| Invariant | Proven by |
| --- | --- |
| SKP-1 | `catalog_retention_is_bounded_by_count_and_bytes`, `dollar_text_is_literal_unless_a_current_skill_selection_binds_it`, `scripts/smoke-tui.py`, crate graph gate |
| SKP-2 | `keyboard_completion_binds_numeric_names_and_token_edits_invalidate_binding`, `completion_from_inside_the_initial_token_preserves_the_request_suffix`, `retry_edit_submission_and_saved_draft_keep_independent_skill_bindings`, `unchanged_numeric_skill_retry_uses_the_historical_semantic_binding`, `numeric_retry_candidate_retains_only_typed_skill_selection` |
| SKP-3 | `escape_preserves_the_query_and_tab_inserts_the_selected_choice`, `mouse_and_wheel_choose_by_name_while_focus_stays_in_the_composer`, `variables_currency_prose_and_command_substitution_do_not_open_the_picker`; `scripts/smoke-tui.py` drives `$`, filtering, Enter/Tab completion and Escape without model work |
| SKP-4 | `short_picker_window_keeps_the_selected_tail_choice_and_controls_visible`, `the_skill_picker_frames_match_their_fixtures`; rendered review above |
