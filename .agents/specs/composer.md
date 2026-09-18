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
draft; `Ctrl-J`, `Shift-Enter` and `Alt-Enter` insert a newline without submission. The message reaches the screen only as the events the runtime emits back. A draft that is
only whitespace submits nothing and is left alone. Rejected: `ratatui-textarea`, which consumes
terminal events when only the router may (INV-1); and the projection appending its own transcript,
which puts two writers on one numbered stream.

**COM-4 — The route is on screen.** The primary composer names its agent and submits a message for
the next turn. A Main-controlled worker window has no input route. After acknowledged Handoff, an
entered worker window names that worker and submits ordinary user input at the boundary its
lifecycle accepts. Selection alone changes neither route (ui-ux §input).
[CCV-1–CCV-4](./child-control-view.md) own controller-aware rendering and routing; authenticated
production snapshot delivery and focused child routing are accepted by `scripts/smoke-delegate.py`.
The product retains an addressed child submission before cold activation or journal work and
returns any later refusal to that same worker composer.

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

EFF-1–EFF-5 own the confirmed reasoning effort, shared RGB rule/label colors and `/effort` selection.
The composer's effort changes only after the addressed idle runtime accepts it.

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

[Named proofs](../evidence/composer.md), one row an invariant.
