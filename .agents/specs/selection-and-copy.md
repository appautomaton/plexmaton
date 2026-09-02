# Spec — Selection and copy

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What a selection is, what copying it returns, and where copied text goes |
| Depends on | The selection and copy rules in [`ui-ux.md`](../ui-ux.md); the `Escape` ladder in [interaction-routing](./interaction-routing.md) INV-6; the virtualization contract in [transcript-layout](./transcript-layout.md) TR-2 |
| Proven by | `plexmaton-tui::state::selection` and `::workspace` tests, `plexmaton-cli::clipboard` tests; see the evidence table |

## Purpose

Plexmaton owns the alternate screen, which takes the terminal's own selection away over every region
it draws. That made an application-owned selection mandatory rather than a nicety, and made its
correctness load-bearing: a copy that returns painted cells returns borders, truncations, and
whatever happened to be on screen — and silently omits everything that was not.

## Invariants

**SEL-1 — A selection names content, never cells.** It is a range over one surface's entries, in
that surface's own order. Scrolling, resizing, re-wrapping, and re-styling therefore cannot change
what is selected or what copying it returns, and a selection extends past the viewport by
construction rather than by a mechanism that has to remember to.

**SEL-2 — Copy returns the producer's source.** The text comes from the projection, which holds what
the producer sent. Not the cells, not the label, not the decorated line: an artifact copies as its
stable pointer, and a message copies as the text of its deltas.

**SEL-3 — One selection, in one surface, for one agent.** A selection carries the surface and the
agent it indexes, so it cannot survive onto a different list. Extending in a different surface
replaces it rather than spanning both, and a selection whose surface stops showing its agent is
**dropped**, not carried across: index three of one agent's messages is a different message in
another's. Carrying the agent alone stops a *frame* highlighting the wrong list but not a *copy*
reading one, and that failure is invisible because the highlight has already gone.

**SEL-4 — The workspace produces copied text and never delivers it.** A copy leaves as a value on
`Outcome`, exactly as a submission does (COM-3). Nothing in `plexmaton-tui` may reach a clipboard,
which is why semantic copy tests do not depend on a host desktop and why the transport can be
replaced without touching the projection.

**SEL-5 — The selection is the feedback.** OSC 52 is unacknowledged: the terminal never replies, and
many terminals and multiplexers decline it unless configured to allow it. So a copy claims nothing.
What the user sees is the selection still highlighted and counted in the surface's title.

## Model

```text
focused surface + its agent ──▶ entries ──▶ Selection { surface, agent, anchor, focus }
                                   │                        │
        content paints entry n ────┘                        └──▶ copy() ──▶ Outcome.copied
        highlighted if in range                                              │
                                                       plexmaton-cli::clipboard (OSC 52)
```

### What an entry is

| Surface | Entries, in order | What one copies as |
| --- | --- | --- |
| Conversation, Inspector | Transcript items | The item's source text |
| Activity | Tool activity, then artifacts, then mail | A tool's label; an **artifact's pointer**; a mail's sender and summary |

The order here is the order the content functions draw, and it has to be: an index meaning different
entries in the two places would select one thing and copy another.

### The grammar

| Input | Meaning |
| --- | --- |
| `Shift-↑` / `Shift-↓` | Extend the selection; with none, select the newest entry |
| `Ctrl-Y` | Copy |
| `Escape` | Drop the selection — the innermost rung of INV-6's ladder |
| `Shift` + any mouse gesture | Hand the gesture to the terminal's own selection (INV-8) |

Both chords resolve before keyboard focus is consulted, for the same reason the inspector's do: the
inspector holds a text input while focused, and its artifacts and mail would otherwise be the one
content in the workspace that cannot be selected.

**Copy is `Ctrl-Y`, not `Ctrl-C`,** because `Ctrl-C` is the unconditional exit (INV-7). A key that
sometimes copies and sometimes ends the session is worse than an unfamiliar one. The cost is real,
and the `Shift` escape hatch to the terminal's own copy is what covers the habit.

**Starting a selection takes the newest entry.** One rule for both kinds of list. The alternative —
start at the end the arrow came from — reads sensibly in a conversation and absurdly in a detail
panel, and everything in this workspace is append-ordered.

## Failure modes

| Situation | Response |
| --- | --- |
| Extending on a surface with no entries | Nothing selected, no repaint |
| Extending past either end | Clamped, like every other list here |
| Copying with nothing selected | No `CopyRequest`; the composition root has nothing to deliver |
| The selected agent leaves the roster | `copy` finds no agent and returns nothing rather than stale text |
| The terminal declines OSC 52 | Undetectable here, and claimed nowhere (SEL-5) |
| A write to the terminal fails | An `io::Error` out of the composition root, like any other terminal write |

## Out of scope

- **Character-granular selection inside an entry.** It needs an inverse map from cells back through
  the wrapping cache to byte offsets, which nothing in the canonical journey asks for, and which
  would make SEL-1 false rather than merely harder to prove.
- **Mouse-driven application selection.** `Shift` hands the pointer to the terminal, which is the
  escape hatch `ui-ux.md` requires and which needs no application mechanism at all. The contract
  asks for a keyboard equivalent of every mouse gesture, not the reverse.
- **A native clipboard crate.** `arboard` reaches the desktop the *process* is on, which over SSH or
  inside tmux is the wrong machine. OSC 52 reaches the terminal the *user* is at. A native path is a
  local-desktop convenience, and it waits for a user whose terminal refuses OSC 52.
- **Paste.** The router already declines `Event::Paste`; nothing in the journey pastes.

## Evidence

| Invariant | Proven by |
| --- | --- |
| SEL-1 | `copy_is_the_same_at_every_width_and_scroll_position`, `copying_returns_the_source_between_the_endpoints` |
| SEL-2 | `copying_an_artifact_returns_its_pointer_rather_than_its_label`, `the_journey_copies_evidence_and_returns_to_the_prior_state` |
| SEL-3 | `escape_clears_the_selection_before_it_closes_the_inspector`, `copying_returns_the_source_between_the_endpoints`, `a_selection_does_not_survive_the_surface_changing_agents` |
| SEL-4 | `copying_writes_a_terminated_osc_52_sequence_carrying_the_encoded_text`, `multi_byte_text_survives_the_encoding` |
| SEL-5 | `escape_clears_the_selection_before_it_closes_the_inspector`, `a_selection_does_not_survive_the_surface_changing_agents` |
