# Spec — Drawer

| Field | Value |
| --- | --- |
| Status | Implemented; evidence below |
| Owns | The workspace's own surface: how it opens, its one geometry, its pages, and what leaves the workspace when a row is chosen |
| Depends on | [`ui-ux.md`](../ui-ux.md) §product vocabulary and §surface model; SURF-3, SURF-4, SURF-5; COM-1 for its filter; [conversation-picker](./conversation-picker.md) for the Conversations page; PER-7 and PER-8 for the Permissions page |
| Proven by | `plexmaton-tui::{layout, workspace, frames}` tests and the executable's tests |

## Invariants

**DRW-1 — One surface for the workspace.** `Ctrl-P` pulls the Drawer open from any focus state,
over a waiting approval included, and never touches a draft: it addresses the workspace, not a
conversation. While open it holds the keyboard and blocks every surface below it (SURF-4), one
layer above the approval, whose card is exactly where it was when the Drawer closes. It holds
pages, never Commands: a `/` in its filter is a character like any other.

**DRW-2 — One geometry.** Docked to the top edge at full width, with the rows its content asks
for, clamped to what lies above the status line. No layout class changes it, and its top edge
stays put while the content height changes. The list asks for at least ten rows so its padding,
and with it the filter's caret row, hold still while a filter empties it (COM-1). Rejected: a
centred overlay capped at 76 columns inside three-cell margins, which shrank abruptly as the
terminal grew and left a short terminal too cramped to list its rows.

**DRW-3 — Pages open in place, and `Escape` returns one layer.** The list names three pages, found
by any part of their names. `↑` / `↓` and the wheel move the marker, stopping at the ends; `Enter`
or a matching press and release on a row hands the page to the composition root as a value, which
owns what opening it costs. The page replaces the list inside the same surface, keeping the list's
filter and choice for its return (SURF-5). `Escape` returns page, list, then origin, one layer per
press. The chosen row carries `Accent` against `Muted`.

**DRW-4 — Configuration shows the model this process resolved.** The composition root projects
provider, wire model ID and reasoning effort from the model handed to the runtime, without keys or
file access in the TUI. The page is read-only and navigated: `↑` / `↓` or `k` / `j` scroll its
values under a footer that keeps its row.

## Model

```text
Ctrl-P ──▶ DrawerIntent::Open ───▶ Drawer { shown: Pages, filter, chosen, return_focus }
Enter  ──▶ DrawerIntent::Choose ─┬─ Pages ─────────▶ Outcome.page          (the root opens it)
                                 ├─ Conversations ─▶ Outcome.conversation  (SPK-2)
                                 └─ Permissions ───▶ Outcome.permission    (PER-7)
root ──▶ show_page(Shown::…) ────▶ the page, in place
Esc    ──▶ drawer_back(): page → Pages → closed, focus back where it was
```

| Fact | Value |
| --- | --- |
| Title | `Workspace`, then ` · <page>`; Configuration adds ` · read only` |
| Kind | `Drawer` while typed into (the list, Conversations); `Modal` while navigated (Configuration, Permissions). One z-index, above approvals |
| Rows | The list: pages + 8, at least 10. Conversations: visible rows + 9. Configuration: three per field + 7. Permissions: PER-7's budget |
| Footer | `↑↓ choose · Enter open · Esc close` on the list; a page ends `Esc back` |

## Failure modes

| Situation | Response |
| --- | --- |
| A filter admitting no page | `No page matches`; `Enter` does nothing |
| `Ctrl-P` while already open | Nothing; the chord is answered |
| A page asked for while the Drawer is closed | The Drawer opens on it, and closing returns focus to the composer |
| A page's result arriving after `Escape` | Ignored: the page it was for is gone (SPK-3, PER-7) |
| A terminal too short for the rows a page asks for | Clamped above the status line; the row under the marker and the footer stay visible |

## Evidence

| Invariant | Proven by |
| --- | --- |
| DRW-1 | `the_chord_pulls_the_drawer_open_and_takes_the_keyboard`, `the_drawer_opens_over_a_waiting_approval_and_leaves_the_card_alone`, `typing_filters_the_list_and_never_reaches_the_composer`, `a_page_is_found_by_its_name_and_a_slash_is_a_character`; `scripts/smoke-tui.py` pulls it open and closes it in a real terminal |
| DRW-2 | `the_drawer_spans_the_width_and_keeps_its_top_edge`, `the_drawer_spans_every_width_with_one_row_per_page`, `short_drawer_and_wheel_use_the_visible_choice_window`, `short_conversation_page_keeps_selected_result_and_footer_visible`, `the_drawer_frames_match_their_fixtures` with the `drawer-*` frames, `the_drawer_filter_paints_its_own_caret_while_editing`, `a_long_drawer_filter_keeps_the_caret_inside_its_row` |
| DRW-3 | `the_list_finds_a_page_by_name_and_hands_it_to_the_root`, `the_escape_ladder_returns_page_then_list_then_origin`, `escape_closes_the_drawer_and_returns_the_keyboard`, `stepping_stops_at_the_ends_of_the_pages`, `a_filter_matching_nothing_leaves_no_choice`, `the_chosen_drawer_row_carries_the_accent_role`; `scripts/smoke-permissions.py` walks the ladder from the Permissions page |
| DRW-4 | `the_configuration_page_shows_the_resolved_model`, `short_configuration_pages_scroll_to_the_remaining_values`, `the_configuration_page_frame_matches_its_fixture` with the `drawer-configuration` frame |
