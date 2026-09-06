# Spec — Composer

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What the workspace's inputs are for: which conversation each addresses, where its cursor comes from, and what submitting does. The editing model itself is `TextInput`, shared by every input |
| Depends on | The locked input decisions in [`ui-ux.md`](../ui-ux.md) §input; focus from [surface-model](./surface-model.md) SURF-3 |
| Proven by | `plexmaton-tui::state::composer`, `::render`, and the executable's tests |

## Invariants

**COM-1 — One cursor, derived.** A text cursor is on screen exactly when the focused surface's kind
holds one (ui-ux §input). The caret is painted by one call per frame, `place_cursor`, for whichever
surface owns it, so "how many cursors are there" is answered by the focus ring rather than by
counting call sites.

The Drawer's filter paints its own caret and leading inset. Its single row scrolls horizontally
around the insertion point, reserving a caret cell; it never borrows the primary draft's position.

**COM-2 — One insertion point, on a grapheme boundary.** Every input is a `TextInput`: text plus one
byte offset that is always on a cluster boundary and never past the end, restored by every mutator so
no caller checks it. Insert, delete, kill and motion act there rather than at the end, and none of
them can split a cluster: `Backspace` after an `e` and a combining acute removes both. Motion is
measured in clusters and logical lines, so it never depends on a width; only presentation does, and
the rows an input paints and the caret it reports come from one wrap. The visible window is the tail
pulled up to contain the caret, computed rather than stored, so no scroll offset can disagree with
where the caret is. Rejected: deriving the caret by measuring painted cells, which can only ever put
it after the last line and is why the draft previously had no insertion point to move.

**COM-3 — Submit is a command, not a write.** Normal submission hands text to the runtime and clears the
draft; the message reaches the screen only as the events the runtime emits back. A draft that is
only whitespace submits nothing and is left alone. Rejected: `ratatui-textarea`, which consumes
terminal events when only the router may (INV-1); and the projection appending its own transcript,
which puts two writers on one numbered stream.

**COM-4 — The route is on screen.** The primary composer names its agent and submits a message for
the next turn; an entered worker window names that worker and submits steering for its next step.
Selection alone changes neither route (ui-ux §input).

**COM-5 — Current work is derived and static.** The conversation's activity line, its last row
above the composer's top rule, shows at most one label: `Approval required` outranks
`Running <tool>`, then `Responding`, then `Thinking`; idle shows none. The label is derived from
the semantic projection and owns no timer; the composer's rules never carry it. The same row's
right end holds the selection note (SEL-5) and the attention pill (ATT-1), which no longer have a
border to ride (ui-ux §input).

**COM-6 — Input selection names editable source.** A click places the caret; dragging retains a
grapheme-boundary anchor, paints the source range, and release copies it through SEL-4. Typing or
deletion replaces that range; motion or `Esc` clears it, and an active drag cancels without copying.
The composer, entered worker input and the Drawer's filter share this model. An edit that joins clusters
repairs the caret against the complete text; an exactly full row reserves the following caret row.
Bracketed terminal paste replaces the selected range atomically, normalizes CRLF/CR to newlines,
and never submits. The Drawer's filter flattens pasted line breaks to spaces to remain single-line.

**COM-7 — Edited retry transfers draft ownership on acknowledgment.** Edit & retry fills the
composer from the addressed question and labels it `Editing previous message · Esc to cancel`.
Submit retains the text until the runtime acknowledges its replacement projection or accepts owned
skill preparation; failure retains or returns it exactly once. Cancel or acknowledgment restores
the displaced draft. No branch mutation occurs while merely editing (JRN-8, SKP-2).

## Model

```text
TextIntent ──▶ ViewState::edit ──▶ Option<Submission>
                     │                  ├─ Message  ──▶ Input::Submitted
                     │                  └─ Steering ──▶ Input::Steered
                     └─ draft, in graphemes                │
                                                          ▼
                                            ConversationEvent stream ──▶ transcript

Ctrl-C ──▶ non-empty draft ──▶ clear
       └─▶ empty draft ──▶ Outcome::interrupted(agent) ──▶ Input::Interrupted
                                                               │
                                           UndeliveredInput ────┴──▶ addressed draft
```

| Fact | Value |
| --- | --- |
| Place | Directly under the primary conversation, between two rules (ui-ux §input): the top rule carries the title and the resolved model's reasoning effort, then the lines, then the bottom rule. The columns a box's sides would spend stay blank, so the caret and the pointer keep a box's geometry |
| Height | One row per wrapped line, at the width of the conversation column it actually occupies, up to a third of the terminal's height and never fewer than three; a taller draft shows the window containing the caret, which is its newest lines until `↑`/`↓` or the wheel over the composer walk the caret out of them |
| Current work | One semantic suffix in the existing divider; action required uses its role and other work is ambient |
| On a short terminal | Served before the notice strip and the agent list: a workspace that cannot be typed into is not a supported shape |
| While a sub-agent's input holds the cursor | One row over the bottom rule, `Message Agent A · ⇥ to return`: no top rule, no title, still a focus stop and a pointer target. `Tab` from that input lands on it, because the composer follows the second window in the focus ring |

## Failure modes

| Situation | Response |
| --- | --- |
| Blank or whitespace-only draft submitted | Nothing is sent and the draft is kept |
| `Ctrl-C` on a non-empty draft | The draft is discarded without interrupting; a later `Ctrl-C` may address the running turn (INV-7) |
| `Backspace` on an empty draft | No change is reported, so it costs no repaint |
| Submitting before any agent exists | The text stays in the draft; there is no session to deliver into |
| Runtime returns text its boundary could not claim | The composition root restores it to the addressed editable draft without inventing a transcript item |
| A text intent arriving under navigation focus | Cannot happen and is not re-checked: the router reads focus from the same state (INV-2) |
| Draft taller than its window | The window follows the caret; `↑`/`↓` and the wheel move it one row at a time and stop at the ends |

## Evidence

| Invariant | Proven by |
| --- | --- |
| COM-7 | `edit_retry_keeps_exact_text_until_ack_and_escape_restores_displaced_draft`, `failed_retry_append_returns_edited_input_without_starting_another_request`, `plain_retry_leaves_edit_mode_and_restores_the_displaced_draft`, `escape_clears_retry_input_selection_before_cancelling_the_edit`, `acknowledged_edit_retry_restores_the_draft_without_losing_keyboard_focus`, `retry_edit_submission_and_saved_draft_keep_independent_skill_bindings`; SKP-2 |
| COM-1 | `the_cursor_exists_only_while_a_text_input_holds_focus`, `no_kind_puts_a_cursor_on_screen_before_the_composer_exists`, `a_wrapped_draft_puts_the_caret_at_the_end_of_the_text_not_on_the_border`, `the_caret_reports_the_row_and_column_it_is_painted_on`, `a_wide_glyph_advances_the_caret_by_two_cells`, `a_click_lands_on_the_boundary_under_it`, `a_draft_reserves_the_rows_it_needs_in_an_ultrawide_split`, `the_drawer_filter_paints_its_own_caret_while_editing`, `a_long_drawer_filter_keeps_the_caret_inside_its_row` |
| COM-2 | `backspace_removes_a_whole_grapheme_cluster`, `deleting_an_empty_draft_changes_nothing`, `editing_happens_at_the_caret_rather_than_at_the_end`, `motion_steps_over_whole_clusters_and_stops_at_the_ends`, `word_deletion_takes_the_trailing_space_and_the_word`, `killing_binds_to_the_logical_line_the_caret_is_on`, `the_visible_window_follows_the_caret_above_the_tail`, `a_draft_grows_to_a_third_of_the_column_then_its_window_follows_the_caret` with the `composer-grown-*` and `composer-windowed-*` frames, `the_wheel_over_the_composer_walks_the_draft_one_row_per_notch` |
| COM-3 | `a_blank_draft_submits_nothing_and_is_left_alone`, `taking_the_draft_returns_it_exactly_and_clears_it`, `returned_text_lands_after_the_existing_draft`, `a_typed_message_reaches_the_transcript_by_way_of_the_runtime`, `a_submitted_message_is_a_finished_user_item`, `a_live_dispatch_restores_undelivered_user_text`, `persistence_failure_restores_the_draft_and_opens_one_notice` |
| COM-4 | `the_composer_names_its_target_while_another_agent_is_selected`, `the_inspectors_input_submits_steering_for_that_agents_next_step`, `production_mapping_preserves_message_steering_interrupt_and_approval` |
| COM-5 | `current_work_priority_is_derived_from_semantic_facts`, `parallel_running_tools_use_stable_first_appearance_order`, `the_activity_line_names_each_current_work_state_and_the_rule_carries_none`, `current_work_does_not_move_input_and_repeated_facts_cost_no_frame`, `the_current_work_frames_match_their_fixtures` with the `current-work-*` frames |
| COM-6 | `pointer_clicks_place_the_caret_in_each_input`, `edits_that_join_clusters_restore_the_grapheme_boundary`, `a_full_input_row_never_places_the_caret_on_the_border`, `terminal_paste_edits_the_focused_input_without_submitting`; `scripts/smoke-tui.py` drives Chinese paste, pointer insertion, selection replacement and source copying |
