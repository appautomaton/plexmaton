# Spec — Drawer

| Field | Value |
| --- | --- |
| Status | Implemented; evidence below |
| Owns | The workspace's own surface: how it opens, its one geometry, its pages, and what leaves the workspace when a row is chosen |
| Depends on | [`ui-ux.md`](../ui-ux.md) §product vocabulary and §surface model; SURF-3, SURF-4, SURF-5; COM-1 for its filter; PER-7 and PER-8 for the Permissions page, which is the Project's |
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

**DRW-3 — Pages open in place, and `Escape` returns one layer.** The list names two pages, found
by any part of their names. `↑` / `↓` and the wheel move the marker, stopping at the ends; `Enter`
or a matching press and release on a row hands the page to the composition root as a value, which
owns what opening it costs. The page replaces the list inside the same surface, keeping the list's
filter and choice for its return (SURF-5). `Escape` returns page, list, then origin, one layer per
press. The chosen row carries `Chosen` across its whole width, a bar with weight and a hue, against `Muted`.
INV-3 shares that choice with pointer movement. A padded bottom-center `︽` handle retracts the whole Drawer to
its origin from any page; Esc retains the one-layer ladder. The eight-cell hit region shares the
existing bottom-border row. Undecorated downward corners retain the side strokes; only the interior is underlined, so no
underline crosses or protrudes beyond a side stroke. Together they form the lower outline;
the single two-cell glyph has equal padding, a muted resting color and accent hover, without animation. INV-11 guards its press/release.

**DRW-4 — Configuration shows the model this process resolved.** The composition root projects
provider, wire model ID and reasoning effort from the model handed to the runtime, without keys or
file access in the TUI. The page is read-only and navigated: `↑` / `↓` or `k` / `j` scroll its
values under a footer that keeps its row.

## Model

```text
Ctrl-P ──▶ DrawerIntent::Open ───▶ Drawer { shown: Pages, filter, chosen, return_focus }
Enter  ──▶ DrawerIntent::Choose ─┬─ Pages ───────▶ Outcome.page        (the root opens it)
                                 └─ Permissions ─▶ Outcome.permission  (PER-7)
root ──▶ show_page(Shown::…) ────▶ the page, in place
Esc    ──▶ drawer_back(): page → Pages → closed, focus back where it was
```

| Fact | Value |
| --- | --- |
| Title | `Workspace`, then ` · <page>`; Configuration adds ` · read only` |
| Kind | `Drawer` while typed into (the list); `Modal` while navigated (Configuration, Permissions). One z-index, above approvals |
| Rows | The list: pages + 8, at least 10. Configuration: three per field + 7. Permissions: PER-7's budget |
| Footer | `↑↓ choose · Enter open · Esc close` on the list; a page ends `Esc back` |

## Failure modes

| Situation | Response |
| --- | --- |
| A filter admitting no page | `No page matches`; `Enter` does nothing |
| `Ctrl-P` while already open | Nothing; the chord is answered |
| A page asked for while the Drawer is closed | The Drawer opens on it, and closing returns focus to the composer |
| A page's result arriving after `Escape` | Ignored: the page it was for is gone (PER-7) |
| A terminal too short for the rows a page asks for | Clamped above the status line; the row under the marker and the footer stay visible |

## Evidence

[Named proofs](../evidence/drawer.md), one row an invariant.

## Rendered controls

Shared choice and the retract control were inspected at
[120](../../crates/plexmaton-tui/frames/interaction/drawer-120.svg),
[88](../../crates/plexmaton-tui/frames/interaction/drawer-88.svg) and
[60](../../crates/plexmaton-tui/frames/interaction/drawer-60.svg) columns;
Configuration's hovered control is
[120](../../crates/plexmaton-tui/frames/interaction/configuration-120.svg),
[88](../../crates/plexmaton-tui/frames/interaction/configuration-88.svg),
[60](../../crates/plexmaton-tui/frames/interaction/configuration-60.svg).
The `interaction_preview` example reproduces these frames.
