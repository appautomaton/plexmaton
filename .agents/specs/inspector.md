# Spec — Inspector

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What an inspector shows, where it goes, and what opening, pinning, resizing, and closing one do |
| Depends on | [surface-model](./surface-model.md) SURF-3 and SURF-5; the Escape ladder in [interaction-routing](./interaction-routing.md) INV-6; the shelf rules in [`ui-ux.md`](../roadmap/ui-ux.md) |
| Proven by | `plexmaton-tui::layout::inspector`, `::state::inspector`, and `::workspace` tests; see the evidence table |

## Purpose

The workspace can show one agent's conversation. The canonical journey needs it to show two — the
one being read and the one being checked on — without the second replacing the first, and without
becoming a window manager to do it.

This is also the phase's one dismissible layer, so it is what gives the `Escape` ladder something to
resolve and what makes "exactly one cursor" a claim that could fail: with a second text input on
screen, focus is the only thing deciding where a keystroke goes.

## Invariants

**INS-1 — Which agent is inspected is its own state.** Opening does not move the selection. An
unpinned inspector follows the selection, so a peek stays a peek at whatever the user is looking at;
a pinned one keeps its agent, which is what puts two different agents on screen at once. That is the
whole of what pinned means.

**INS-2 — The conversation keeps ten readable rows, or the inspector takes the region outright.**
The rule is about what an inspector *takes*: on a terminal with fewer than ten rows of conversation
before anything opened, it may take none. There is no third outcome where a shelf and a squeezed
conversation share a region too small for both.

**INS-3 — Presentation is derived from size, never stored.** Shelf, column, and maximized are
chosen per frame from the layout class and the user's maximize. Changing presentation changes no
identity, no scroll position, and no focus, because there is nothing to change — the surface is the
same surface at a different size.

**INS-4 — Opening focuses it; closing gives focus back.** Opening is an explicit action, so its
input is usable without a second step (D-026). `Escape` closes it and returns focus to the
conversation, but only when the inspector was holding focus: closing an overlay elsewhere must not
take the cursor out of the composer.

**INS-5 — The steer input exists only while the inspector holds focus.** There is nothing to
mistarget because there is nothing there (D-018). Its rows come out of the inspector's own budget,
never the conversation's guarantee (D-022), and while it is active the primary composer collapses to
a single row that stays clickable and stays a focus stop (D-027).

## Model

```text
ViewState                          layout::inspector
  Inspector { agent, pinned,  ──▶  presentation(class, maximized, rows)
              maximized, rows }         │
                                        ├─ Shelf      docked to the top of the conversation
  selection ──── follow ────▶           ├─ Column     the secondary column, at ultrawide
                (unpinned only)         └─ Maximized  the whole conversation region
```

### Ownership

| Fact | Owner | Why not elsewhere |
| --- | --- | --- |
| Which agent is inspected | `state::inspector::Inspector` | User intent, and it has to survive frames and selection changes |
| Which presentation is used | `layout::inspector`, per frame | Derived from size; storing it would let a stored value disagree with the terminal |
| Whether a press began a resize | `state::inspector::Inspector` | The router owns *that* a gesture is in progress (INV-4); what the gesture means is not its business |
| Which agent a keystroke addresses | Derived from the focused surface | Two answers to "where does this go" is how a steer reaches the wrong worker |
| The draft itself | `ViewState`, keyed by agent | A draft belongs to the conversation it addresses, so peeking elsewhere and returning finds it |

### Geometry

A shelf takes `min(⌊0.55 × region⌋, region − 10)` rows from the top of the conversation, or whatever
height the user dragged to, clamped the same way. Below eighteen rows of conversation region the
presentation falls back to maximized.

`ui-ux.md` gives the reason for eighteen as the ten-row guarantee failing. It does not fail — below
eighteen the guarantee simply binds instead of the share, and the shelf shrinks toward nothing while
the conversation keeps its ten. What stops being true is that the shelf is worth being one: at
eighteen rows it is already two borders and six lines. The number is kept as locked; the reason is
corrected here.

Regions are **split, never stacked**. A shelf that both took ten rows and covered them would be
counted twice by the tiling check and would need a z-index to hit-test. Nothing in this workspace
overlaps, which is why z-order promotion has no caller here (see Out of scope).

### The grammar

`ui-ux.md` left the bindings to be decided by the prototype and asked for one coherent grammar
rather than one binding per widget. Every chord resolves before keyboard focus is consulted, which
is what makes them reachable while the inspector's own input holds the cursor.

| Input | Meaning | Available under |
| --- | --- | --- |
| `Enter` | Open the selected agent's inspector | Navigation focus only — under a cursor it submits (INV-2) |
| `Escape` | Close it, returning focus to the conversation | Both |
| `Ctrl-P` | Toggle pin | Both |
| `Ctrl-F` | Toggle maximize | Both |
| `Ctrl-Shift-↓` / `Ctrl-Shift-↑` | Move the bottom edge one row | Both |
| Drag the bottom edge | The same, with pointer capture | Pointer |

## Failure modes

| Situation | Response |
| --- | --- |
| An inspector command with nothing open | A no-op that does not advance the revision. The router says what was pressed; whether there is anything to act on is the reducer's question |
| The inspected agent leaves the roster | The panel says so rather than painting an empty box. Nothing removes an agent in Phase 00; this is the prepared answer |
| A drag that began on the body, not the edge | Moves nothing. A grab is recorded at press time or not at all |
| A press on another surface while a grab is held | Clears the grab rather than leaving a stale one for the next drag |
| A height dragged past the guarantee | Clamped to it. Dragging is a choice inside the contract, never a way out |
| A conversation region too small for two surfaces | Maximized, not a sliver |
| Focus preferring an inspector that is not registered | Falls back to the first ring stop, and reclaims focus if it returns (SURF-5) |

## Out of scope

- **Z-order promotion as a user action.** There is one inspector and the regions are split, so no
  two surfaces compete for a cell. The mechanism arrives with the surface that gives it a second
  sibling; until then `SurfaceTreeError::ZOrderExhausted` stays unreachable, the same way SURF-2's
  clipping and SURF-4's modality do.
- **Free two-axis drag.** Cut by D-028 before this step: a shelf is docked, so only its height is a
  user choice.
- **Expand and collapse for tool activity.** A transcript-item behaviour, not an inspector one.
- **A second full conversation at ultrawide.** The inspector becomes the secondary column and
  carries a conversation among other things; three live transcripts is the monitoring layout D-024
  already declined.

## Evidence

| Invariant | Proven by |
| --- | --- |
| INS-1 | `an_unpinned_inspector_follows_the_selection_and_a_pinned_one_stays`, `a_pinned_inspector_keeps_its_agent_while_the_conversation_moves_on`, `re_pointing_at_another_agent_keeps_the_presentation_the_user_chose` |
| INS-2 | `an_open_inspector_leaves_ten_readable_rows_or_takes_the_region_outright`, `a_dragged_height_is_clamped_rather_than_obeyed`, `dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle` |
| INS-3 | `presentation_follows_the_terminal_and_the_users_maximize`, `the_composer_survives_every_presentation`, `registered_surfaces_tile_the_terminal_without_gaps_or_overlap` |
| INS-4 | `enter_opens_the_inspector_and_escape_returns_focus_to_the_conversation`, `the_inspector_grammar_is_the_same_under_both_focus_modes_except_enter` |
| INS-5 | `the_inspector_takes_the_cursor_and_the_composer_keeps_one_row`, `only_a_press_on_the_bottom_edge_starts_a_resize`, `the_keyboard_moves_the_inspectors_edge_the_same_way_the_pointer_does` |
