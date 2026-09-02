# Spec — The second window

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What the second window shows, where it goes, and what opening, entering, resizing, and closing it do |
| Depends on | [surface-model](./surface-model.md) SURF-3 and SURF-5; the Escape ladder in [interaction-routing](./interaction-routing.md) INV-6; the shelf rules in [`ui-ux.md`](../roadmap/ui-ux.md) |
| Proven by | `plexmaton-tui::layout::inspector`, `::state::inspector`, and `::workspace` tests; see the evidence table |

The code calls this surface `Inspector`, which is the name `ui-ux.md` gave it. On screen and in
this document it is the second window: the user is talking to one agent and looking at another.

## Purpose

The workspace shows the primary agent's conversation. The canonical journey needs it to show a
second one — the agent being checked on — without the first leaving the screen, and without
becoming a window manager to do it.

This is also the phase's one dismissible layer, so it is what gives the `Escape` ladder something to
resolve and what makes "exactly one cursor" a claim that could fail: with a second text input on
screen, focus is the only thing deciding where a keystroke goes.

## Invariants

**INS-1 — The window is the selection.** The primary's conversation is always on screen, and the
list holds only the sub-agents. Selecting one — by arrow, by click in the list, or by going to its
request — opens its conversation over or beside the primary's; `Escape` clears the selection and
closes it. Nothing stores "which agent is open" separately from the selection, so the two cannot
disagree and no conversation is ever on screen twice. There is no pin and no follow: the window
stays while the user types to the primary, until they press `Escape` (D-049).

**INS-2 — The conversation keeps ten readable rows beneath the window, or the window takes the
region outright.** The rule is about what the window *covers*: on a terminal with fewer than ten
rows of conversation before anything opened, it may cover none. There is no third outcome where a
shelf and a squeezed conversation share a region too small for both.

**INS-3 — Presentation is derived from size, never stored.** Shelf, column, and maximized are
chosen per frame from the layout class and the user's maximize. Changing presentation changes no
identity, no scroll position, and no focus, because there is nothing to change — the surface is the
same surface at a different size.

**INS-4 — Entering is explicit; closing gives focus back.** Looking at an agent does not move the
keyboard: the arrows keep working in the list. `Enter` moves the keyboard into the window, so its
input is usable without a second step (D-026). Closing returns focus to the conversation, but only
when the window was holding it: closing an overlay elsewhere must not take the cursor out of the
composer.

**INS-5 — The window's input exists only while the window holds focus.** There is nothing to
mistarget because there is nothing there (D-018). It takes a strip off the bottom of the window's
own rectangle, never off the conversation's guarantee (D-022), and while it is active the primary
composer collapses to a single row that stays clickable and stays a focus stop (D-027). A rectangle
with no room for both keeps the conversation and shows no input, which is the same all-or-nothing
rule the row budget uses.

**INS-6 — What the window shows is a conversation, and in Phase 00 that is all it shows.** The
looked-at agent's, virtualized through the same cache and the same reading position the main
conversation uses (TR-1, TR-3, TR-5) — so the two scroll independently because their readers are
keyed by agent, not because a second mechanism was added. It is not a second copy of the activity
column: that column already shows the selected agent's tools, artifacts and mail.

The composed surface `ui-ux.md` and `plexmaton.md` describe — conversation, tools, mail, artifacts
and status in one place — is **deferred to Phase 03, not weakened** (D-046). The activity lives in
the agent column, stacked under the list (D-014), so it stays on screen beside the second window at
every width that has a column at all.

**INS-7 — A window with no room for its input is a navigation surface.** INS-5 says a rectangle
that cannot hold both keeps the conversation and shows no input; this is the other half of that
sentence. With no input drawn there is no cursor, no text target, and no draft to type into, and the
same geometry answers all three — one function, called by the renderer and by focus, so the
affordance, the caret and the keystroke cannot reach different conclusions (D-047).

## Model

```text
Roster                              layout::inspector
  selected ≠ primary  ──▶ open ──▶  presentation(class, maximized, rows)
  selected = primary  ──▶ closed         │
                                         ├─ Shelf      docked to the top of the conversation
state::inspector                         ├─ Column     the secondary column, at ultrawide
  { maximized, rows }  presentation      └─ Maximized  the whole conversation region
```

### Ownership

| Fact | Owner | Why not elsewhere |
| --- | --- | --- |
| Which agent is in the window, and whether one is open | `state::roster::Roster::peeked`, derived from the selection | A stored copy is a second source of truth; the first implementation had one and showed a conversation twice |
| Where the second column goes at ultrawide | `layout::inspector`, out of the conversation's width | The agent column is one box holding the list and the activity (D-014), and it is not what a second conversation takes rows or columns from |
| How the user asked for it to be shown | `state::inspector::Inspector` | Maximize and a dragged height belong to the window, not to the agent it happens to show |
| Which presentation is used | `layout::inspector`, per frame | Derived from size; storing it would let a stored value disagree with the terminal |
| Whether a press began a resize | `state::inspector::Inspector` | The router owns *that* a gesture is in progress (INV-4); what the gesture means is not its business |
| Which agent a keystroke addresses | Derived from the focused surface, and from whether its input fits | Two answers to "where does this go" is how a message reaches the wrong worker, or a draft nobody can see (INS-7) |
| Where the window's input goes | `layout::inspector::steer_split`, per frame | Geometry, and a draw call must not be where a surface decides which of two states it is in |
| The draft itself | `ViewState`, keyed by agent | A draft belongs to the conversation it addresses, so looking elsewhere and returning finds it |

### Geometry

A shelf takes `min(⌊0.55 × region⌋, region − 10)` rows from the top of the conversation, or whatever
height the user dragged to, clamped the same way. Below eighteen rows of conversation region the
presentation falls back to maximized.

`ui-ux.md` gives the reason for eighteen as the ten-row guarantee failing. It does not fail — below
eighteen the guarantee simply binds instead of the share, and the shelf shrinks toward nothing while
the conversation keeps its ten. What stops being true is that the shelf is worth being one: at
eighteen rows it is already two borders and six lines. The number is kept as locked; the reason is
corrected here.

A shelf **floats**: it is the one surface above the base layer, drawn inside the conversation's
border so the conversation keeps its whole rectangle, its title, and its reading position, and the
rows it covers are the top of the interior — empty rows or rows already read, because a
conversation shorter than its panel sits at the bottom the way an overflowing one does. The base
layer still tiles the terminal; the pointer and the painter both resolve the topmost surface at a
cell, and the cells beneath the shelf are cleared before it paints so nothing shows through.

### The grammar

`ui-ux.md` left the bindings to be decided by the prototype and asked for one coherent grammar
rather than one binding per widget. The chords resolve before keyboard focus is consulted, which is
what makes them reachable while the window's own input holds the cursor.

| Input | Meaning | Available under |
| --- | --- | --- |
| `↑` / `↓` in the list, or a click on a row | Look at that sub-agent: open the window on it, or move it to them | Navigation focus (the list) |
| `Enter` | Enter the window, so its input takes the keyboard | Navigation focus only — under a cursor it submits (INV-2) |
| `Escape` | Close it, returning focus to the conversation | Both |
| `Ctrl-F` | Toggle maximize | Both |
| `Ctrl-Shift-↓` / `Ctrl-Shift-↑` | Move the bottom edge one row | Both |
| Drag the bottom edge | The same, with pointer capture | Pointer |

## Failure modes

| Situation | Response |
| --- | --- |
| A window command with nothing open | A no-op that does not advance the revision. The router says what was pressed; whether there is anything to act on is the reducer's question |
| The looked-at agent leaves the roster | The panel says so rather than painting an empty box. Nothing removes an agent in Phase 00; this is the prepared answer |
| Going to a request from the primary itself | No window opens and any open one closes, because the primary is already on screen; the keyboard is pointed at its conversation |
| No sub-agents yet | The list says so and the arrows move nothing |
| A drag that began on the body, not the edge | Moves nothing. A grab is recorded at press time or not at all |
| A press on another surface while a grab is held | Clears the grab rather than leaving a stale one for the next drag |
| A height dragged past the guarantee | Clamped to it. Dragging is a choice inside the contract, never a way out |
| A conversation region too small for two surfaces | Maximized, not a sliver |
| A window rectangle too short for a conversation and an input | Keeps the conversation and becomes navigation-only: no input, no cursor, no draft (INS-7). It still holds focus, and giving the rows back gives the input back |
| Closing while a selection was made in the window | The selection goes with it (SEL-3), and so does the maximize or dragged height; a reopened window is the default one |
| Focus preferring a window that is not registered | Falls back to the first ring stop, and reclaims focus if it returns (SURF-5) |

## Out of scope

- **Z-order promotion as a user action.** There is one surface above the base layer, so nothing
  ever needs to be raised past a sibling. The mechanism arrives with a second floating surface;
  until then `SurfaceTreeError::ZOrderExhausted` stays unreachable, the same way SURF-2's clipping
  and SURF-4's modality do.
- **Free two-axis drag.** Cut by D-028 before this step: a shelf is docked, so only its height is a
  user choice.
- **Expand and collapse for tool activity.** A transcript-item behaviour, not a window one.
- **Tools, mail, artifacts and status inside the window.** Phase 03's, with the reasoning in
  D-046. The activity column owns them for the selected agent.
- **A third live conversation at ultrawide.** The window becomes the secondary column, and D-024
  allows exactly one; three live transcripts is the monitoring layout it already declined.

## Evidence

| Invariant | Proven by |
| --- | --- |
| INS-1 | `the_window_floats_over_the_primary_and_escape_closes_it`, `clicking_an_agent_in_the_list_selects_it_and_opens_its_window`, `the_journey_reaches_two_agents_without_losing_the_first`, `the_journey_keeps_a_second_agent_on_screen_and_takes_a_request_without_being_interrupted` |
| INS-2 | `an_open_inspector_leaves_ten_readable_rows_or_takes_the_region_outright`, `a_dragged_height_is_clamped_rather_than_obeyed`, `dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle` |
| INS-3 | `presentation_follows_the_terminal_and_the_users_maximize`, `the_composer_survives_every_presentation`, `registered_surfaces_tile_the_terminal_without_gaps_or_overlap`, `the_presentation_survives_the_window_showing_another_agent` |
| INS-4 | `selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation`, `the_inspector_grammar_is_the_same_under_both_focus_modes_except_enter` |
| INS-5 | `the_inspector_takes_the_cursor_and_the_composer_keeps_one_row`, `only_a_press_on_the_bottom_edge_starts_a_resize`, `the_keyboard_moves_the_inspectors_edge_the_same_way_the_pointer_does` |
| INS-6 | `two_conversations_scroll_independently_and_neither_moves_the_other`, `an_inspected_conversation_keeps_its_own_reading_position_across_a_close_and_reopen`, `the_journey_reaches_two_agents_without_losing_the_first`, `a_conversation_drawn_at_two_widths_measures_correctly_at_both` |
| INS-7 | `an_inspector_too_short_for_its_input_takes_no_typing_and_no_cursor`, `an_inspector_splits_for_its_input_only_when_both_still_fit` |
