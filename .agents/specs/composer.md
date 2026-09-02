# Spec — Composer

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | The one text input: its editing model, where its cursor comes from, and what submitting does |
| Depends on | The locked input decisions in [`ui-ux.md`](../ui-ux.md) §input; focus from [surface-model](./surface-model.md) SURF-3 |
| Proven by | `plexmaton-tui::state::composer`, `::render`, and the executable's tests |

## Invariants

**COM-1 — One cursor, derived.** A text cursor is on screen exactly when the focused surface's kind
holds one (ui-ux §input). The caret is painted by one call per frame, `place_cursor`, for whichever
surface owns it, so "how many cursors are there" is answered by the focus ring rather than by
counting call sites.

**COM-2 — Edits are graphemes.** Insert and delete operate on grapheme clusters: `Backspace` after
an `e` and a combining acute removes both, and no operation leaves the draft split mid-cluster. The
draft has no stored insertion point, because no binding moves one; it is the end of the text.

**COM-3 — Submit is a command, not a write.** Submitting hands text to the runtime and clears the
draft; the message reaches the screen only as the events the runtime emits back. A draft that is
only whitespace submits nothing and is left alone. Rejected: `ratatui-textarea`, which consumes
terminal events when only the router may (INV-1); and the projection appending its own transcript,
which puts two writers on one numbered stream.

**COM-4 — The route is on screen.** The primary composer names its agent and submits a message for
the next turn; an entered worker window names that worker and submits steering for its next step.
Selection alone changes neither route (ui-ux §input).

## Model

```text
TextIntent ──▶ ViewState::edit ──▶ Option<Submission>
                     │                  ├─ Message  ──▶ Input::Submitted
                     │                  └─ Steering ──▶ Input::Steered
                     └─ draft, in graphemes                │
                                                          ▼
                                            SessionEvent stream ──▶ transcript

Ctrl-C ──▶ Outcome::interrupted(agent) ──▶ Input::Interrupted
```

| Fact | Value |
| --- | --- |
| Place | The bottom section of the primary conversation's box (ui-ux §input): a divider carrying the title, the lines, and the box's bottom edge |
| Height | Up to three lines; a longer draft shows its newest lines, the bounded tail the notice strip also uses |
| On a short terminal | Served before the notice strip and the agent list: a workspace that cannot be typed into is not a supported shape |
| While a sub-agent's input holds the cursor | One row closing the box, `Message Agent A · ⇥ to return`: no divider, no title, still a focus stop and a pointer target. `Tab` from that input lands on it, because the composer follows the second window in the focus ring |

## Failure modes

| Situation | Response |
| --- | --- |
| Blank or whitespace-only draft submitted | Nothing is sent and the draft is kept |
| `Ctrl-C` on a draft | The draft is discarded, its conversation is named for interruption, and nothing quits (INV-7) |
| `Backspace` on an empty draft | No change is reported, so it costs no repaint |
| Submitting before any agent exists | The text stays in the draft; there is no session to deliver into |
| A text intent arriving under navigation focus | Cannot happen and is not re-checked: the router reads focus from the same state (INV-2) |
| Draft longer than the visible lines | The newest lines show, because that is where the cursor is |

## Evidence

| Invariant | Proven by |
| --- | --- |
| COM-1 | `the_cursor_exists_only_while_a_text_input_holds_focus`, `no_kind_puts_a_cursor_on_screen_before_the_composer_exists` |
| COM-2 | `backspace_removes_a_whole_grapheme_cluster`, `deleting_an_empty_draft_changes_nothing` |
| COM-3 | `a_blank_draft_submits_nothing_and_is_left_alone`, `taking_the_draft_returns_it_exactly_and_clears_it`, `a_typed_message_reaches_the_transcript_by_way_of_the_runtime`, `a_submitted_message_is_a_finished_user_item` |
| COM-4 | `the_composer_names_its_target_while_another_agent_is_selected`, `the_inspectors_input_submits_steering_for_that_agents_next_step`, `production_mapping_preserves_message_steering_and_interrupt` |
