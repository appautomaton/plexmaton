# Spec — Selection and copy

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What a selection is, what copying it returns, and where copied text goes |
| Depends on | The rules and what they rejected in [`ui-ux.md`](../ui-ux.md) §selection and copy; the `Escape` ladder in [interaction-routing](./interaction-routing.md) INV-6; the virtualization contract in [transcript-layout](./transcript-layout.md) TR-2 |
| Proven by | `plexmaton-tui::state::selection` and `::workspace` tests, `plexmaton-cli::clipboard` tests |

## Invariants

**SEL-1 — A selection names content, never cells.** One explicit range is either keyboard-selected
entries or pointer-selected visible text. Pointer endpoints carry entry identity, order and a
grapheme-boundary offset or MTH-1 atomic extent validated by the exact text prefix and formula
range; scrolling and reflow preserve them. Pure append outside a complete atom preserves endpoints;
a changed prefix or formula interpretation invalidates the range rather than copying a
different slice. A keyboard selection started with none selects the newest entry.

The pointer retains its anchor on press, selects only after movement, and requests automatic copy on
release. PRE-4 keeps missing preparation pending without holding input or clearing the range.
Partial endpoints can span entries, including disclosed tool text. An empty released range
copies nothing and is cleared. A plain message click clears selection without copying; a formula
click selects, highlights and copies its complete original delimited TeX. Either drag direction
expands every formula intersection to the entire rectangle, including blank and edge cells.

Editable inputs select source offsets under COM-6. They use the same copy boundary, with zero
transcript entries in the resulting `CopyRequest`.

**SEL-2 — Copy follows the selected representation.** The Copy icon and keyboard entry ranges read
the producer's source: an artifact copies as its stable pointer and a message as the text of its deltas,
never the cells, the label, or the decorated line. A tool copies its retained invocation followed
by its retained outcome, omitting whichever is absent and joining both with one newline. Disclosure
headings, gutters, clipping, and omission labels are presentation and never enter that source.
This original-source path preserves Markdown delimiters, fences, link targets and table source.
Pointer ranges instead slice a deterministic plain-text projection. Soft wraps add no copy
newlines; code indentation and semantic line breaks remain; entries join with a blank line.
Table cells use tabs/newlines in row-major order, without grid padding or repeated narrow-view
labels. Fragments map text offsets to drawn columns; headings for code/diagnostics, borders,
buttons and recovery feedback supply no text ranges. No copy path reads terminal cells.
MTH-1 formula fragments keep exact delimiters and whitespace in that plain-text projection;
capability/failure labels are not copied. Clipping and source fallback preserve the whole atom.
Maps and prepared rows share MD-4's bounded cache; cache hits borrow mapping data, and highlighting
copies only the rows it paints. Copying reads only the selected entries, not the whole history;
PRE-4 prepares missing maps through the owned worker rather than reparsing on release. Pending,
changed-source and failure feedback appears on the selected conversation's activity line, not as a delivery acknowledgement.

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
accepted the request. Native helper failure is returned without a silent route change. The CLI publishes `✓ Copied` only
for native acceptance, or `Copy sent` for terminal/tmux send. A two-second receipt overlays the
bottom-right status cells without layout or focus changes; quit has priority without extending
its window. New admitted copies withdraw old receipts; cancellation, replacement and delivery
failure produce none. The two tmux legs remain independent: a successful terminal send can report
`Copy sent` even when the helper rejects; this does not claim clipboard acceptance. All helper
operations bound both stdin writes and exit waits to 500 ms, kill and reap on failure, timeout or
cancellation, and bound cleanup to another 500 ms. Cleanup failure prevents replacement; kill-on-drop
is only a final guard. The screen claims no stronger delivery than the route establishes.

**SEL-6 — A held drag reaches entries beyond the viewport.** While a conversation holds capture,
the content row beside its top or bottom chrome starts a bounded scroll rate on one monotonic timer;
the chrome and the first row beyond it increase that rate. Each wake moves that conversation and
extends the semantic range; moving inward, release, cancel, a lost-button bare move, or the content
boundary disarms it without another frame. Losing terminal focus pauses the timer while retaining
the semantic selection and pointer capture; a later drag resumes from the same anchor.

**SEL-7 — Message actions are separate from content selection.** A text entry reserves a small
right gutter; hover reveals its first-row Nerd Font Copy glyph without changing height or text
width. Its screen column is anchored to the viewport, independent of role, source spans or gutters.
Gentle boundary rules use existing blank separator rows only, never cover content or create
spacing. The button has padded hit geometry and accent hover, and emits exact source only on
an unchanged press/release. Dragging away cancels it; clipping, resize and focus loss cannot leave
an invisible action active. Retry buttons use separate muted/accent spans, never selection reversal.
Repeated hover is free. Rejected: copying on a plain message click and reversing an entire action
row, which makes actions indistinguishable from a retained selection.

**SEL-8 — Clipboard work cannot monopolize interaction.** The CLI polls one retained helper future
beside input and frame deadlines, with at most one latest pending source; replacement cancels and
reaps the old child before starting any newer delivery effect. Helpers own no terminal writer,
idle delivery owns no wake, and shutdown cancels/reaps active work without starting pending work.

## Model

```text
retained entry ──┬─ original source ─────────────▶ Copy icon / keyboard entry range
                └─ text layout ─┬─ styled rows ─▶ conversation surface
                                ├─ offset map ──▶ pointer text range / highlight
                                └─ visible text ▶ plain-text copy
                                                        │
                                                  Outcome.copied
                                                        │
                                               CLI clipboard transport
```

Clipboard admission checks each source's allocated capacity against 8 MiB before any effect or
cancellation; refusal preserves prior accepted work and never truncates source. At most two admitted
source allocations are retained. OSC 52 is written serially on the interaction loop, not by an
independent task; slow terminal writes remain a separate output-boundary concern. The retained
helper future survives a losing `select` poll, so a partial stdin write cannot be restarted.

| Surface | Entries, in order | What one copies as |
| --- | --- | --- |
| Conversation, Inspector | Text, tool, artifact and mail entries in first-appearance order | Pointer: selected visible text. Keyboard entries: source, retained tool details, artifact pointer, or mail recipient and summary |

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
| Streaming reinterprets text before an endpoint | Clear the range and pending drag; never copy a shifted substring |
| A keyboard-selected tool has no retained invocation or outcome | It contributes no source; its painted label is never substituted |
| The selected agent leaves the roster | `copy` finds no agent and returns nothing rather than stale text |
| The outer terminal declines OSC 52 | Undetectable here, and claimed nowhere (SEL-5) |
| Local macOS rejects `pbcopy` or exceeds its deadline | Error returned to the composition root; no unacknowledged fallback reported as success |
| A newer copy arrives during helper work | Cancel/reap the active child, coalesce pending requests to the newest exact source, then deliver it |
| Copy source allocation exceeds its ceiling | Explicit admission error before terminal output or cancellation; no silent truncation |
| The CLI leaves while copying | Cancel/reap the helper; discard pending work before terminal restoration |
| Helper cleanup cannot establish a reaped child | Report cleanup failure and refuse any replacement |
| tmux is absent, rejects `load-buffer -w`, or takes too long | The request has a deadline; its child is killed and reaped before return, while the already-attempted OSC 52 route remains |
| A held drag reaches the viewport boundary | The timer disarms; repeated wakeups cost no frame |
| A write to the terminal fails | An `io::Error` out of the composition root, like any other terminal write |

## Evidence

[Named proofs](../evidence/selection-and-copy.md), one row an invariant.

## Rendered feedback

SEL-5 composition and Ctrl-J drafts were inspected at
[120](../../crates/plexmaton-tui/frames/interaction/copied-120.svg),
[88](../../crates/plexmaton-tui/frames/interaction/copied-88.svg) and
[60](../../crates/plexmaton-tui/frames/interaction/copied-60.svg) columns.
The other transport's final row is
[120](../../crates/plexmaton-tui/frames/interaction/sent-120.svg),
[88](../../crates/plexmaton-tui/frames/interaction/sent-88.svg),
[60](../../crates/plexmaton-tui/frames/interaction/sent-60.svg);
quit precedence is
[120](../../crates/plexmaton-tui/frames/interaction/quit-120.svg),
[88](../../crates/plexmaton-tui/frames/interaction/quit-88.svg),
[60](../../crates/plexmaton-tui/frames/interaction/quit-60.svg).
Reproduce with `cargo run -p plexmaton-tui --example interaction_preview -- target/interaction-review`.
