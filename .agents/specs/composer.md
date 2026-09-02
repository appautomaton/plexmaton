# Spec — Composer

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | The one text input: its editing model, where its cursor comes from, and what submitting does |
| Depends on | The locked input decisions in [`ui-ux.md`](../ui-ux.md) §input; focus from [surface-model](./surface-model.md) SURF-3 |
| Proven by | `plexmaton-tui::state::composer`, `::render`, and the executable's tests; see the evidence table |

## Purpose

One place accepts typed text, and one place decides what happens when the user presses `Enter`.
Without that, "where does this keystroke go" becomes invisible state, and a message sent to the
wrong worker is not undone by sending another one.

## Invariants

**COM-1 — One cursor, derived.** A text cursor is on screen exactly when the focused surface's kind
holds one. Nothing else may place one, so "how many cursors are there" is answered by the focus ring
rather than by counting call sites (ui-ux §input).

**COM-2 — Edits are graphemes.** Insert and delete operate on grapheme clusters. `Backspace` after
an `e` and a combining acute removes both, and no operation can leave the draft split mid-cluster.

**COM-3 — Submit is a command, not a write.** Submitting hands text to the runtime and clears the
draft. The message reaches the screen only as the events the runtime emits back. The projection
never writes its own transcript, because a second writer is how a transcript and its runtime begin
to disagree. A draft that is only whitespace submits nothing and is left alone.
Rejected: `ratatui-textarea`, which consumes terminal events when only the router may (INV-1);
and the projection appending its own transcript, which puts two writers on one numbered stream.

**COM-4 — The target is on screen.** The composer's title names the agent it addresses, and that
agent does not change when the selection does (ui-ux §input).

## Model

```text
TextIntent ──▶ ViewState::edit ──▶ Option<String>  ──▶ RuntimeCommand::SendMessage
                     │                (a submission)              │
                     └─ draft, in graphemes                       ▼
                                                     PrototypeEvent stream ──▶ the transcript
```

### Ownership

| Fact | Owner | Why not elsewhere |
| --- | --- | --- |
| The draft text | `state::composer::Composer` | One string; there is nothing else to keep in sync with it |
| Whether a cursor exists | `SurfaceKind`, via the focused surface | A second answer is how a workspace ends up with none or two |
| Where the caret is painted | `place_cursor`, called once per frame from the render loop for whichever surface owns the cursor | Ratatui hides the cursor unless a frame asks, so one call site *is* the invariant |
| What a submitted message becomes | `plexmaton-sim::Runtime` | The transcript has one writer, and it is the event stream |

### No cursor offset

The draft has no stored insertion point. The router's key grammar binds no cursor movement, so the
insertion point is always the end of the text. An offset nothing can change would be a field to
maintain and a second thing able to disagree with the string. It arrives with the binding that moves
it, not before.

### Height and place

The composer is the bottom section of the primary conversation's box (`ui-ux.md` §input): a
divider carrying its title, its lines, and the box's bottom edge. It asks layout for that divider
and edge plus its line count, capped at three lines, and shows the newest lines when the draft is
longer — the same bounded tail the notice strip uses. On a terminal too short for everything, the
composer is served before the notice strip and the agent list: a workspace that cannot be typed
into is not one of the supported shapes.

While a sub-agent's input holds the cursor the composer is **one row** closing the box — `Message
Agent A · ⇥ to return`, no divider, no title — so the conversation gains one row and nothing else
moves (ui-ux §input). The row is still a focus stop and a pointer target, and `Tab` from the sub-agent's
input lands on it because the composer follows the second window in the focus ring.

## Failure modes

| Situation | Response |
| --- | --- |
| Blank or whitespace-only draft submitted | Nothing is sent and the draft is kept; discarding it would lose typing to a keystroke |
| `Backspace` on an empty draft | No change is reported, so it does not cost a repaint |
| Submitting before any agent exists | The text stays in the draft. There is no session to deliver into, and dropping it would lose it silently |
| A text intent arriving under navigation focus | Cannot happen, and is not re-checked. The router reads focus from the same state (INV-2); a second check would be a second source of truth |
| Draft longer than the visible lines | The newest lines show, because that is where the cursor is. A real viewport arrives with step 4 |

## Out of scope

- **Cursor movement, selection, and editing beyond the four verbs.** They arrive with the bindings
  that need them; the key grammar in [`interaction-routing`](./interaction-routing.md) is the gate.
- **Steering by explicit address** (`@agent-b …`). Locked in `ui-ux.md`; its consumer is the
  delegation record in Phase 03.
- **What the runtime does with a message.** The runtime's; the composer hands over text and a
  target.

## Evidence

| Invariant | Proven by |
| --- | --- |
| COM-1 | `the_cursor_exists_only_while_a_text_input_holds_focus`, `no_kind_puts_a_cursor_on_screen_before_the_composer_exists` |
| COM-2 | `backspace_removes_a_whole_grapheme_cluster`, `deleting_an_empty_draft_changes_nothing` |
| COM-3 | `a_blank_draft_submits_nothing_and_is_left_alone`, `taking_the_draft_returns_it_exactly_and_clears_it`, `a_typed_message_reaches_the_transcript_by_way_of_the_runtime`, `a_submitted_message_is_a_finished_user_item` |
| COM-4 | `the_composer_names_its_target_while_another_agent_is_selected` |
