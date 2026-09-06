# Spec — Composer menu

| Field | Value |
| --- | --- |
| Status | Implemented; verified offline and at three rendered widths |
| Owns | What the primary composer's draft completes to: Skills for `$`, Commands for `/`, and `/resume`'s saved conversations; what leaves the workspace when a row is accepted |
| Depends on | SKL-2/SKL-4/SKL-5, COM-1/COM-3/COM-6, INV-1/INV-6, SURF-3; [conversation-picker](./conversation-picker.md) for `/resume`'s rows; CPL-9 for `/compact` |
| Proven by | TUI, runtime and agent proofs below; real terminal completion smoke |

## Invariants

**SKP-1 — Discovery is a projection.** The primary composer consumes a bounded user-invocable
catalog supplied by the composition root; its menu performs no filesystem or runtime operation.
Names, descriptions and origin labels refer to the runtime's winning skill definitions (SKL-2).

**SKP-2 — Completion is an edit with identity.** Choosing a skill inserts `$name ` without sending
the message and binds only that exact initial token. Input handoff and failure retain the selected
name alongside original text; changing the token invalidates its binding (COM-3/COM-6).

**SKP-3 — The composer keeps input ownership.** The central interaction layer routes menu
navigation, acceptance, pointer selection and scrolling; ordinary typing retains the composer's
caret. Escape closes the menu without deleting the draft; worker inputs do not offer this menu.

**SKP-4 — The menu fits the conversation.** The menu is a titled rule and its rows above the
primary composer's top rule, bounded within its conversation column and available terminal
space. It clips summaries and scrolls choices without obscuring the input or taking another
conversation's space.

**CMD-1 — A Command runs from the conversation that typed it.** `/` lists the Commands; accepting
one leaves the workspace as a value the composition root runs: `/new` and `/resume` as a
`ConversationRequest`, `/compact` as a `CommandRun` whose target is the composer's agent, captured
at acceptance. The runtime admits or refuses the run; a refusal is one sentence after the
conversation's last entry (CPL-9), never a redirect to another conversation. Rejected: a target of
conversation, head and revision revalidated before dispatch, because acceptance and the run are one
loop step and the runtime's admission already names a busy conversation.

**CMD-2 — Only a whole Command runs.** `Tab` completes the chosen Command into the draft as
`/name ` and runs nothing; `Enter` runs a draft that is exactly a Command, with the menu open or
dismissed. `/resume` keeps the text after it as its query. Any other draft with text after the
token, `/compact please` included, is text and submits as text. `/` followed by a character no
Command starts with lists nothing.

## Grammar

At the start of a primary draft, `$` opens available skills and `/` the Commands; subsequent
characters filter. Up/Down select, Tab or Enter completes a skill, Tab completes a Command and
Enter accepts it, and Escape dismisses. A completed `$name request` submits normally on the next
Enter. Exact unselected nonnumeric skill names activate only when present in the user-invocable
catalog. Unknown variables, `$HOME`, currency, command substitutions and dollar expressions inside
prose/code remain literal text. Numeric skill names can be deliberately selected from the menu;
unbound `$100` remains currency. There is no `/skill:` execution alias.

The menu shows up to five choices and keeps the selected row and controls visible when height is
constrained; it is suppressed if even one choice and the controls cannot fit. Source labels precede
truncatable descriptions. Displayed metadata is flattened to inert single-line text while its
semantic source remains unchanged. Editing within the initial token can complete without changing
the request suffix. Dismissal survives caret motion until that token changes.

Selected names accompany submitted, returned and retry-editor input. A historical numeric skill
selection comes from its typed journal activation, never from interpreting currency-shaped text.

## Rendered review

Skills: the real workspace buffer was inspected at
[wide](../spikes/agent-skills/frames/skill-picker-wide.svg),
[medium](../spikes/agent-skills/frames/skill-picker-medium.svg) and
[narrow](../spikes/agent-skills/frames/skill-picker-narrow.svg) widths. Reproduce with
`cargo run -p plexmaton-tui --example skill_picker_preview -- target/skill-picker-preview`.
Commands and `/resume`: the `composer-menu-commands-medium`, `composer-menu-resume-loading-medium`
and `composer-menu-resume-medium` frames, cut from the real buffer above the composer.

## Evidence

| Invariant | Proven by |
| --- | --- |
| SKP-1 | `catalog_retention_is_bounded_by_count_and_bytes`, `dollar_text_is_literal_unless_a_current_skill_selection_binds_it`, `scripts/smoke-tui.py`, crate graph gate |
| SKP-2 | `keyboard_completion_binds_numeric_names_and_token_edits_invalidate_binding`, `completion_from_inside_the_initial_token_preserves_the_request_suffix`, `retry_edit_submission_and_saved_draft_keep_independent_skill_bindings`, `unchanged_numeric_skill_retry_uses_the_historical_semantic_binding`, `numeric_retry_candidate_retains_only_typed_skill_selection` |
| SKP-3 | `escape_preserves_the_query_and_tab_inserts_the_selected_choice`, `mouse_and_wheel_choose_by_name_while_focus_stays_in_the_composer`, `variables_currency_prose_and_command_substitution_do_not_open_the_picker`, `the_listing_follows_the_leading_token` |
| SKP-4 | `short_picker_window_keeps_the_selected_tail_choice_and_controls_visible`, `the_skill_picker_frames_match_their_fixtures`; rendered review above |
| CMD-1 | `the_slash_lists_the_commands_and_only_a_whole_command_runs`, `a_requested_compaction_shows_on_the_activity_line_and_ends_with_a_note`; the runtime's CPL-9 proofs. A loopback run of `/compact` through the executable is unproven |
| CMD-2 | `the_slash_lists_the_commands_and_only_a_whole_command_runs`, `tab_completes_a_command_and_escape_keeps_the_draft`, `paste_and_unicode_inside_the_token_follow_the_same_rule`, `a_whole_draft_is_a_command_only_when_nothing_else_is_in_it` |
