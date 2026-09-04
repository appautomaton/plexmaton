# Spec — Selection and copy

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What a selection is, what copying it returns, and where copied text goes |
| Depends on | The rules and what they rejected in [`ui-ux.md`](../ui-ux.md) §selection and copy; the `Escape` ladder in [interaction-routing](./interaction-routing.md) INV-6; the virtualization contract in [transcript-layout](./transcript-layout.md) TR-2 |
| Proven by | `plexmaton-tui::state::selection` and `::workspace` tests, `plexmaton-cli::clipboard` tests |

## Invariants

**SEL-1 — A selection names content, never cells.** A transcript selection is a range over one conversation's entries,
in first-appearance order, so scrolling, resizing, re-wrapping, and re-styling cannot change what
is selected or what copying returns, and it extends past the viewport by construction. A selection
started with none selects the newest entry, because everything here is append-ordered.

The pointer makes the same range: a press anchors on the entry under it, a drag carries the moving
end, and a press where there is no entry clears it. Every entry, not only a foldable one — the
pointer addressing a narrower set than the keyboard is what made a click select a tool row and do
nothing at all on the message beside it, over a screen whose mouse this process had taken.

Editable inputs select source offsets under COM-6. They use the same copy boundary, with zero
transcript entries in the resulting `CopyRequest`.

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

**SEL-5 — Delivery follows the user's terminal boundary without inventing evidence.** Local macOS
uses the system `pbcopy` helper with UTF-8, except when SSH, a multiplexer, or an embedded editor
terminal is detected. Other direct terminals receive OSC 52; immediate tmux receives its DCS passthrough
envelope and an owned, bounded `load-buffer -w` request. An embedded editor terminal keeps plain
OSC 52 while retaining the tmux leg. No remote host clipboard is treated as the user's. A
successful terminal write has no acknowledgement; helper success proves only that the helper
accepted the request. Native helper failure is returned without a silent route change. All helper
operations bound both stdin writes and exit waits to 500 ms, kill and reap on failure or timeout,
and enable kill-on-drop for cancellation. The screen claims no stronger delivery than the route establishes.

**SEL-6 — A held drag reaches entries beyond the viewport.** While a conversation holds capture,
the content row beside its top or bottom chrome starts a bounded scroll rate on one monotonic timer;
the chrome and the first row beyond it increase that rate. Each wake moves that conversation and
extends the semantic range; moving inward, release, cancel, a lost-button bare move, or the content
boundary disarms it without another frame. Losing terminal focus pauses the timer while retaining
the semantic selection and pointer capture; a later drag resumes from the same anchor.

## Model

```text
focused surface + its agent ──▶ entries ──▶ Selection { surface, agent, anchor, focus }
                                   │                        │
        content paints entry n ────┘                        └──▶ copy() ──▶ Outcome.copied
        highlighted if in range                                              │
                                                       plexmaton-cli::clipboard
                                                          ├─ local macOS pbcopy
                                                          ├─ direct OSC 52
                                                          └─ tmux DCS + load-buffer -w
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
| The outer terminal declines OSC 52 | Undetectable here, and claimed nowhere (SEL-5) |
| Local macOS rejects `pbcopy` or exceeds its deadline | Error returned to the composition root; no unacknowledged fallback reported as success |
| tmux is absent, rejects `load-buffer -w`, or takes too long | The request has a 500 ms deadline; its child is killed and reaped before return, while the already-attempted OSC 52 route remains |
| A held drag reaches the viewport boundary | The timer disarms; repeated wakeups cost no frame |
| A write to the terminal fails | An `io::Error` out of the composition root, like any other terminal write |

## Evidence

| Invariant | Proven by |
| --- | --- |
| SEL-1 | `copy_is_the_same_at_every_width_and_scroll_position`, `copying_returns_the_source_between_the_endpoints`, `copying_a_conversation_preserves_interleaved_entry_sources`, `ctrl_o_opens_the_selections_focus_entry_in_place_at_each_drawn_width`, `dragging_across_a_conversation_selects_and_copies_what_it_crossed` |
| SEL-2 | `tool_copy_preserves_every_retained_source_in_producer_order`, `tool_copy_is_identical_when_compact_open_resized_scrolled_and_monochrome`, `copying_a_conversation_preserves_interleaved_entry_sources`, `copying_an_artifact_returns_its_pointer_rather_than_its_label`, `the_journey_copies_evidence_and_returns_to_the_prior_state` |
| SEL-3 | `escape_clears_the_selection_before_it_closes_the_inspector`, `copying_returns_the_source_between_the_endpoints`, `a_selection_does_not_survive_the_surface_changing_agents` |
| SEL-4 | `tool_copy_is_identical_when_compact_open_resized_scrolled_and_monochrome`, `direct_copy_writes_the_exact_terminated_osc_52_sequence` |
| SEL-5 | `direct_copy_writes_the_exact_terminated_osc_52_sequence`, `tmux_copy_escapes_the_inner_sequence_inside_one_dcs_envelope`, `tmux_delivery_names_the_outer_clipboard_flag_and_stdin`, `route_detection_requires_a_non_empty_tmux_identity`, `an_editor_terminal_keeps_tmux_delivery_but_receives_plain_osc_52`, `native_copy_requires_an_unambiguous_local_macos_terminal`, `native_copy_uses_the_system_helper_with_utf8`, `clipboard_helper_receives_exact_unicode_source_and_eof`, `clipboard_helper_rejection_is_not_reported_as_delivery`, `clipboard_deadline_bounds_a_blocked_stdin_pipe`, `clipboard_deadline_also_bounds_waiting_after_eof`; local macOS/iTerm clipboard delivery manually confirmed by the user on 2026-09-04 at `96917a4`; cancellation reaping remains unproven |
| SEL-6 | `an_edge_drag_scrolls_and_copies_entries_that_started_off_screen`, `drag_autoscroll_activates_on_the_content_row_beside_chrome` |
