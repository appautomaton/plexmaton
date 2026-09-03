# Spec — Selection and copy

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What a selection is, what copying it returns, and where copied text goes |
| Depends on | The rules and what they rejected in [`ui-ux.md`](../ui-ux.md) §selection and copy; the `Escape` ladder in [interaction-routing](./interaction-routing.md) INV-6; the virtualization contract in [transcript-layout](./transcript-layout.md) TR-2 |
| Proven by | `plexmaton-tui::state::selection` and `::workspace` tests, `plexmaton-cli::clipboard` tests |

## Invariants

**SEL-1 — A selection names content, never cells.** It is a range over one conversation's entries,
in first-appearance order, so scrolling, resizing, re-wrapping, and re-styling cannot change what
is selected or what copying returns, and it extends past the viewport by construction. A selection
started with none selects the newest entry, because everything here is append-ordered.

**SEL-2 — Copy returns the producer's source.** The text comes from the projection, which holds what
the producer sent: an artifact copies as its stable pointer and a message as the text of its deltas,
never the cells, the label, or the decorated line. A tool copies its retained invocation followed
by its retained outcome, omitting whichever is absent and joining both with one newline. Disclosure
headings, gutters, clipping, and omission labels are presentation and never enter that source.

**SEL-3 — One selection, in one surface, for one agent.** A selection carries the surface and the
agent it indexes. Extending in a different surface replaces it, and a selection whose surface stops
showing its agent is dropped, because index three of one agent's messages is a different message in
another's, and a copy reading the wrong list is invisible once the highlight has gone.

**SEL-4 — The workspace produces copied text and never delivers it.** A copy leaves as a value on
`Outcome`, as a submission does (COM-3); nothing in `plexmaton-tui` reaches a clipboard, so semantic
copy tests need no host desktop and the transport is replaceable.

**SEL-5 — The selection is the feedback.** OSC 52 is unacknowledged and many terminals and
multiplexers decline it unless configured, so a copy claims nothing; the user sees the selection
still highlighted and counted in the surface's title.

## Model

```text
focused surface + its agent ──▶ entries ──▶ Selection { surface, agent, anchor, focus }
                                   │                        │
        content paints entry n ────┘                        └──▶ copy() ──▶ Outcome.copied
        highlighted if in range                                              │
                                                       plexmaton-cli::clipboard (OSC 52)
```

| Surface | Entries, in order | What one copies as |
| --- | --- | --- |
| Conversation, Inspector | Text, tool, artifact and mail entries in first-appearance order | Text source; a tool's retained invocation then outcome; an artifact pointer; or a mail recipient and summary |

The order is `AgentView::entries()`, which is also what transcript measurement and content consume,
so one index names one entry in every path. The bindings are in the routing spec's key grammar; the
selection chords resolve before keyboard focus is consulted, so the window's artifacts and mail can
be selected while its input holds the cursor.

## Failure modes

| Situation | Response |
| --- | --- |
| Extending on a surface with no entries | Nothing selected, no repaint |
| Extending past either end | Clamped |
| Copying with nothing selected | No `CopyRequest`; the composition root has nothing to deliver |
| A selected tool has no retained invocation or outcome | It contributes no source; its painted label is never substituted |
| The selected agent leaves the roster | `copy` finds no agent and returns nothing rather than stale text |
| The terminal declines OSC 52 | Undetectable here, and claimed nowhere (SEL-5) |
| A write to the terminal fails | An `io::Error` out of the composition root, like any other terminal write |

## Evidence

| Invariant | Proven by |
| --- | --- |
| SEL-1 | `copy_is_the_same_at_every_width_and_scroll_position`, `copying_returns_the_source_between_the_endpoints`, `copying_a_conversation_preserves_interleaved_entry_sources`, `ctrl_o_opens_the_selections_focus_entry_in_place_at_each_drawn_width` |
| SEL-2 | `tool_copy_preserves_every_retained_source_in_producer_order`, `tool_copy_is_identical_when_compact_open_resized_scrolled_and_monochrome`, `copying_a_conversation_preserves_interleaved_entry_sources`, `copying_an_artifact_returns_its_pointer_rather_than_its_label`, `the_journey_copies_evidence_and_returns_to_the_prior_state` |
| SEL-3 | `escape_clears_the_selection_before_it_closes_the_inspector`, `copying_returns_the_source_between_the_endpoints`, `a_selection_does_not_survive_the_surface_changing_agents` |
| SEL-4 | `tool_copy_is_identical_when_compact_open_resized_scrolled_and_monochrome`, `copying_writes_a_terminated_osc_52_sequence_carrying_the_encoded_text`, `multi_byte_text_survives_the_encoding` |
| SEL-5 | `escape_clears_the_selection_before_it_closes_the_inspector`, `a_selection_does_not_survive_the_surface_changing_agents` |
