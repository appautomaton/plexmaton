# Plexmaton UI/UX Contract

| Field | Value |
| --- | --- |
| Applies to | Every delivery phase |
| Parent roadmap | [Plexmaton Roadmap](./roadmap.md) |

This document is the product experience, stated as rules. It is the destination: which of it
exists today is the active phase's business, and nothing here tracks that. A rule's mechanism, its
bindings, and its measurements live in the spec that owns it; this file keeps the rule and, where
the rule was contested, what it rejected. A layout is locked only after a rendered frame at wide,
medium, and narrow has been looked at.

## Experience promise

Plexmaton is an interactive multi-agent workspace. The user converses with the primary agent,
observes delegated work, inspects evidence, and steers or stops agents without losing spatial
context, scroll position, or control of the active conversation.

Detail is revealed progressively:

1. Ambient status shows that work exists.
2. A lightweight peek reveals what an agent is doing.
3. Looking at another agent keeps it on screen, above or beside the primary's conversation, for
   as long as the user wants; the primary never leaves.
4. A maximized session exposes the complete transcript and artifacts.

Background work stays observable without becoming foreground noise.

## Product vocabulary

These terms are used identically in product copy, architecture, code, and tests.

| Term | Meaning |
| --- | --- |
| Agent | A running or resumable model-driven worker with explicit lifecycle and capabilities |
| Session | The durable conversation and work record owned by one agent identity |
| Turn | One admitted unit of model or tool activity within a session |
| Mail | A typed, durable message delivered between sessions |
| Artifact | Durable work product or evidence, referenced by identity or path rather than copied into mail |
| Surface | A rendered interactive region that participates in z-order and event routing |
| Viewport | The independently scrollable visible window over content owned by a surface |
| Inspector | The second window: one agent's conversation, shown over or beside the primary's while the user looks at that agent. `Inspector` is the code's name; user-facing copy names the agent |
| Peek | Looking at a sub-agent in the list, which opens the second window; `Escape` closes it. The primary is not in the list, because its conversation is the screen |

An alias such as `B` or `reviewer` is a display label, never durable identity. A pane is a layout
presentation, not a session.

## Locked interaction decisions

Viewport and surface code cannot be written without these, and changing one afterwards is a
rewrite rather than an adjustment.

### Screen ownership: full alternate screen

Plexmaton owns the alternate screen for its whole session and restores it on exit, including on
error and on panic. It does not render into inline scrollback. Rejected: inline scrollback, which
gives the terminal ownership of scroll position and contradicts per-surface scroll ownership, while
every locked surface behaviour, floating windows, z-order, pointer capture, independent viewports
and hit testing, needs a coordinate space the application controls completely.

Consequences that are requirements:

- Terminal-native selection is unavailable over owned regions, so the application-owned selection
  model and its modifier escape hatch are mandatory.
- Diagnostics and logs never write to the owned screen. They go to a file or an inspectable surface.
- Restoration survives panic and signal paths, because a leaked alternate screen destroys the user's
  scrollback.

### Input: exactly one cursor

At any moment the workspace shows exactly one text cursor, and typed text can only reach it. Every
other rule about input follows from this one.

- The composer is bound to the **primary agent** and is never retargeted by selection. Its title
  names its target. Selecting, inspecting, or scrolling another agent does not change where typing
  goes. Rejected: one composer whose target follows the selection. The target is invisible state,
  and a misdirected instruction to a running worker is not undone by sending another.
- A sub-agent's input **does not render at all** unless that agent's surface holds keyboard focus.
  There is nothing to mistarget because there is nothing there.
- **Entering a sub-agent's window focuses it; looking at one does not.** Selecting a sub-agent in
  the list opens its window and leaves the keyboard in the list, so the arrows keep moving through
  it. `Enter`, or a click in the window, moves the keyboard in, and its input appears then and is
  usable at once. This does not conflict with "background agents never steal focus": that rule
  constrains what agents do on their own, not what the user asks for. `Escape` closes the window and
  returns focus to the primary conversation. Rejected: focusing on look, which stops the arrows.
- **Every input lives inside the box of the conversation it addresses.** The primary composer is the
  bottom section of the primary's box, under a divider; a sub-agent's input is the bottom of its
  window. There is no input anywhere else, and `Tab` from a sub-agent's input lands on the primary
  composer, which is what the collapsed row's `⇥ to return` promises.
- While a sub-agent's input is active, the primary composer **collapses to a single row** reading
  `Message Agent A · ⇥ to return`, which stays clickable and stays a focus stop. Rejected: hiding
  it, which costs the affordance and jumps the tail of the transcript three rows; one row of jump is
  acceptable and zero costs too much screen on a small terminal.
- A sub-agent's input takes its rows from its **own** surface. It may never consume the rows
  guaranteed to the primary conversation: focusing a worker never squeezes the primary off screen.
- **The composer's bottom border is the status line.** It says one thing at a time: at rest, the
  working directory; after a key that raised a question, the answer, until the next key. Nothing
  else on screen lists keys. Rejected: a key-hint strip along the bottom, a row of chords nobody
  read that cost the conversation a line.
- **Quitting is `Ctrl-D` twice.** The first press makes the status line say so, and any other key
  withdraws it. `Ctrl-C` is the shell's interrupt: it clears the draft under the cursor, and with
  nothing to clear it points at `Ctrl-D`. It never quits.

Making the input physically live inside the surface it addresses turns "where does this keystroke
go" into a fact on screen rather than something to remember. Steering by explicit address
(`@agent-b …`) from the primary composer remains a keyboard path; it produces the same typed
steering intent with the target recorded in the message, so it introduces no hidden state either.

### Nested scrolling: no propagation from an exhausted child

A wheel event routes to the topmost eligible viewport under the pointer and is consumed there. When
that viewport is already at its boundary, the event stops; it does not pass to the parent. A
viewport that cannot scroll at all is not eligible, so the event routes to the next eligible one
beneath it. "Exhausted" and "not scrollable" are deliberately different cases.

Rejected: propagation. It would make a viewport's behaviour depend on its scroll position, so the
same gesture over the same cell would sometimes move a different surface, the exact spatial-memory
failure this contract exists to prevent, and it contradicts the rule that a popup consumes its own
scroll without moving the transcript behind it.

## Locked UX principles

### User control

- Background agents never steal keyboard focus, change the selected transcript, open a surface, or
  scroll a viewport.
- New mail and state changes notify; they do not navigate on the user's behalf.
- Every mouse interaction has a discoverable keyboard equivalent.
- Destructive or high-impact actions identify their target agent or workspace before confirmation.

### Attention management

- Ambient activity, new information, action required, and failure are distinguished. They never
  share one generic notification treatment. Background progress is ambient; completed mail is new
  information; approval or clarification is action required; a failed or disconnected agent is
  failure.
- Background agents never open modal prompts over the user's active work.
- Action-required items enter a visible, ordered Attention queue. The user chooses when to go to the
  requesting agent, unless an already-focused action blocks the current command.
- Repeated updates from one agent coalesce into one attention item.
- Acknowledging a notification is distinct from resolving the underlying mail, approval, or failure.
- The queue carries both directions of the relationship: an agent asking the user for something, and
  a delegating agent objecting to something the user changed. Neither may open a modal or take focus.
- The queue is chrome, like the notice strip: it appears when it has something to say, takes no
  focus, and is never a surface in the sense of §user control.

### Stable spatial memory

- Each surface preserves its own focus and scroll state while hidden, moved, or covered.
- Opening the second window does not disturb the primary transcript's scroll anchor.
- Resize recomputes layout while preserving the semantic anchor in each visible transcript.
- Closing the top surface restores the previous focus predictably.

### Progressive disclosure

- Default views emphasize the current conversation and agent state, not raw infrastructure events.
- Tool calls are compact by default and expose live status, result, and full output on demand.
- Mail presents sender, recipient, purpose, outcome, and durable pointers before transcript detail.
- Context, token, and provider diagnostics are available without occupying permanent screen space.

### Responsive interaction

- Input, scrolling, focus changes, and surface opening stay responsive while models stream and
  tools run.
- Streaming updates do not re-layout content outside the affected visible blocks.
- A background agent may update an ambient status indicator without forcing a full-screen redraw.
- Loading and failure states appear in the affected surface and freeze nothing else.

### Readability

- Visual hierarchy comes from spacing, alignment, restrained colour, and a consistent component
  grammar before decorative borders.
- Agent, mail, tool, reasoning, artifact, warning, and error content are distinguishable without
  relying on colour alone.
- Typeset math is the primary presentation; source is an interaction layer for inspect and copy, and
  a clear failure representation.
- Colour is twelve semantic roles. Widgets name a role, never a terminal colour. A palette is a
  complete assignment of the roles; the shipped palettes are presets, and a new colourway is a new
  assignment, not a constructor and not a widget edit. The default is `ansi`, so the user's terminal
  theme wins.

### Selection and copy

- Mouse capture never makes transcript, tool output, paths, mail, or equations uncopyable.
- A selection is a range over a surface's *entries*, never a rectangle of cells, so it extends past
  the viewport by construction and copying is unaffected by width, scroll position, and decoration.
- Copy is an explicit keyboard action that returns the semantic source: an equation's exact source,
  an artifact's stable value rather than its truncated label, never border glyphs or clipped cells.
- The mouse reaches the terminal's own selection through a modifier escape hatch.
- Delivery goes to the clipboard at the user's terminal, not the machine the process runs on.
- Rejected: character selection, which changes what is copied at a second width; a local clipboard
  crate, which reaches the wrong machine over SSH; and `Ctrl-C` as copy, which is the interrupt.

The mechanism and the bindings are [`specs/selection-and-copy.md`](./specs/selection-and-copy.md).

## Information architecture

The product areas, arranged without assuming they are all permanently visible:

- Primary conversation and composer
- Agent navigator and agent lifecycle status
- Agent inspector: the agent's conversation
- Tool output and diff inspection
- Mail
- Session, context, and provider diagnostics
- Command palette and help
- Permission, approval, and confirmation surfaces

The inspector is the inspected agent's **conversation**. Tool activity, mail and artifacts are
entries in the conversation of the agent that produced them (§progressive disclosure, §transcript
grammar); until that grammar lands, the activity column is their interim home. A composed surface
beside a conversation, with status and an artifact index, is Phase 03's. The other areas are placed
provisionally until the phase that builds them.

## Surface model

Every interactive surface has a stable identity, a kind and owner, a rectangle and a clipping
rectangle, a z-order, visibility and modality, focusability, its own viewport and scroll state
where applicable, drag and resize state where it floats, minimum and preferred sizes, and a
responsive fallback.

| Category | Behaviour |
| --- | --- |
| Base workspace | Primary layout; never floats above other surfaces |
| Second window | The sub-agent the user is looking at, floating over the primary's conversation or beside it at ultrawide; stays while they work elsewhere and closes on `Escape` |
| Modal | Owns input until resolved or dismissed; nothing beneath receives pointer events |
| Popover or menu | Anchored to an initiating element; closes on outside interaction or `Escape` |
| Tooltip | Informational only; never owns keyboard focus |
| Attention queue | Ordered action-required items; opening one is explicit and never caused by background focus theft |

### Shelf: overlay without occlusion

Below ultrawide, the second window is a **shelf** docked to the top edge of the conversation. It
floats: the conversation beneath keeps its whole rectangle, its title, and its reading position, and
a conversation shorter than its panel sits at the bottom, so the shelf covers only empty rows or
rows already read. Transcripts follow their tail, so covering the top hides what has been read and
covering the middle or bottom hides what the user is reading now. Rejected: a centred floating
window, and splitting the region, which moved the conversation under it.

- The primary conversation keeps at least ten readable rows beneath the shelf.
- The composer is never covered, at any size.
- A region too short to hold both maximizes the window instead of drawing a sliver.
- Which agent is shown is the selection. Shelf, column, or maximized is geometry chosen by terminal
  width and the user's maximize, and changing it changes no identity, scroll position, or focus.
  There is no pin: the window stays until `Escape`.

The geometry, the bindings, and what opening means are [`specs/inspector.md`](./specs/inspector.md).

### Drag scope

The only direct manipulation is dragging a shelf's bottom edge to change its height, clamped so the
ten-row guarantee holds, with a keyboard equivalent. A shelf is docked, so its position is
determined and only its height is a user choice. Rejected: free two-axis movement and eight-way
resize, which add pointer capture on both axes, boundary clamping, resize recovery, and keyboard
equivalents for each, for a gesture nothing in the journey needs. Free drag returns when a maximized
surface has a reason to be somewhere other than where the layout puts it.

## Input and event-routing contract

- Pointer events route to the topmost visible surface whose clipped hit region contains the event.
- Wheel events use hover routing: the topmost eligible viewport under the mouse scrolls without
  changing keyboard focus, and a consumed wheel event scrolls only its target.
- Drag begins with pointer capture and continues to the captured surface until release or
  cancellation, even if the pointer leaves its rectangle.
- Keyboard input routes to the focused surface; hover alone never redirects it.
- `Escape` resolves the topmost dismissible interaction, one layer per press, and never quits.
- Modal surfaces block pointer and keyboard delivery to surfaces below them.
- Focus order and command availability are inspectable for keyboard-only use.

The translation, the capture state machine, the key grammar, and the invariants are
[`specs/interaction-routing.md`](./specs/interaction-routing.md). Bindings are tested as one
grammar, never assigned widget by widget.

## The canonical journey

1. Agent A streams in the primary conversation.
2. A delegates a bounded task to agent B.
3. B appears as running without taking focus from A or the composer.
4. The user looks at B, which opens B's conversation over A's.
5. B's conversation streams inside an independently scrollable viewport.
6. The user returns to A and continues typing while B stays on screen.
7. The user resizes B's window while A remains independently usable.
8. B requests approval or clarification; the request enters the Attention queue without opening a
   modal or stealing focus.
9. B sends typed mail to A; ambient status changes without automatic navigation.
10. The user goes to the request, follows an artifact, copies evidence, and returns to the exact
    prior viewport positions.
11. The user steers, stops, dismisses, reopens, or maximizes B through explicit actions.

Every layout class preserves the meaning of this journey even when it changes where surfaces go.

## Responsive layout classes

| Class | Product expectation | Threshold |
| --- | --- | --- |
| Ultrawide | Two conversations side by side; a second agent earns a column rather than an overlay | width ≥ 132 |
| Wide | Agent column plus one conversation; a second agent arrives as a shelf | 96 ≤ width < 132 |
| Medium | The primary conversation dominates; activity compresses to markers | 72 ≤ width < 96 |
| Narrow | One major surface at a time; looking at an agent is a full-region transition, the window's maximized presentation | width < 72 |
| Too small | One explicit notice, never a clipped workspace | width < 48 or height < 12 |

- The agent column sits on the left and holds the list of sub-agents over their activity in one
  box. It is a column from wide up and a band below. Rejected: an activity column on the right,
  stacked under the conversation at medium, which left the screen whenever a second window opened.
- Ultrawide is 132 because two 52-cell conversations and a 28-cell agent column need it, and 52
  cells is roughly where prose stops wrapping awkwardly. It holds exactly one secondary column,
  replaced on selection. Rejected: three live transcripts, which is a monitoring product rather than
  a working one.
- Below 48 × 12 the screen is one notice. Rejected: a clipped workspace.

Resize preserves the focused semantic item, keeps bottom-follow only for viewports already following
the tail, keeps every viewport's anchor, clamps an inaccessible floating surface back into view, and
reflows from source data rather than cropping stale strings.

## Transcript grammar

Each of these has one visual treatment, readable in monochrome and low-colour terminals; colour
carries identity and status but is never the only carrier:

- User message
- Assistant message, streaming and final
- Reasoning summary
- Tool call: queued, running, succeeded, failed, cancelled, approval required
- Diff and artifact
- Agent mail
- Delegation amendment: the user redirected a worker, shown in the delegator's transcript so the
  user and the delegating agent read the same story
- Agent objection: the delegating agent disputes an amendment, shown as action required
- Undelivered steering: a message that never reached its worker, with its original text intact
- System and runtime notice
- Warning and error
- Typeset display math and source reveal

## State matrix

Each applicable surface has an intentional representation for: empty, loading, streaming, idle,
waiting on a tool, model, permission or descendant, paused, completed, failed, cancelled,
disconnected or reconnecting, stale or unavailable persisted content, a capability-degraded
terminal, a delegation amended by the user with the delegator not yet informed, a delegating agent
objecting, steering queued for a worker's next turn boundary, and steering undeliverable with its
payload retained.

## Performance budgets

Targets are chosen against a 16 ms frame, so a budget spent is a frame the user waits for. Work
counts are asserted by tests; wall-clock time is only reported, beside the machine that produced
it (FR-4). The observed figures are in [`specs/frame-loop.md`](./specs/frame-loop.md) §cost.

| Budget | Target | Workload |
| --- | --- | --- |
| Input event to visible frame | 5 ms | `streaming delta` |
| Wheel event to visible scroll | 5 ms | `wheel` |
| Surface open and close | 5 ms, and no re-wrapping | `open inspector` |
| Two conversations on screen, either scrolled | 5 ms, and no re-wrapping | `two conversations` |
| Extending a selection | 5 ms, and no re-wrapping | `extend selection` |
| Opening a conversation nothing has measured | 20 ms | `cold open`, `open hidden conversation` |
| Resize recovery | 20 ms | `resize` |
| Streaming redraw frequency | One frame per changed projection, never per event | FR-1 |
| Layout work per updated transcript block | One item wrapped, at any history length | `streaming delta` |
| Memory retained per hidden conversation | One cache entry per message per width drawn, at most two widths | `open inspector` |

## Open questions

- Persistent agent rail versus a command-driven agent switcher at medium widths.
- How much tool activity remains visible in a collapsed transcript block.
- Notification treatment for mail that arrives while its sender's window is open.
- Whether ten rows is the right primary-conversation guarantee under real transcripts.

An answered question moves into the section that owns the answer.
