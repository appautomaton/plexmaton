# Plexmaton UI/UX Contract

| Field | Value |
| --- | --- |
| Status | Working draft |
| Applies to | Every delivery phase |
| Parent roadmap | [Plexmaton Roadmap](./plexmaton.md) |

This document defines the product experience across phases. It separates durable interaction principles from temporary layouts and implementation details. A layout becomes locked only after the experience skeleton demonstrates it at wide, medium, and narrow terminal sizes.

## Experience promise

Plexmaton is an interactive multi-agent workspace. The user should be able to converse with the primary agent, observe delegated work, inspect evidence, and steer or stop agents without losing spatial context, scroll position, or control of the active conversation.

The interface reveals detail progressively:

1. Ambient status shows that work exists.
2. A lightweight peek reveals what an agent is doing.
3. A pinned inspector supports sustained comparison.
4. A maximized session exposes the complete transcript and artifacts.

Background work must remain observable without becoming foreground noise.

## Product vocabulary

Use these terms consistently in product copy, architecture, and tests:

| Term | Meaning |
| --- | --- |
| Agent | A running or resumable model-driven worker with explicit lifecycle and capabilities |
| Session | The durable conversation/work record owned by one agent identity |
| Turn | One admitted unit of model/tool activity within a session |
| Mail | A typed, durable message delivered between sessions |
| Artifact | Durable work product or evidence referenced by identity/path rather than copied into mail |
| Surface | A rendered interactive region participating in z-order and event routing |
| Viewport | The independently scrollable visible window over content owned by a surface |
| Inspector | A surface exposing one agent/session's transcript, tools, mail, artifacts, and state |
| Peek | A lightweight, dismissible inspector presentation |
| Pin | Promote a transient inspector into persistent workspace layout |

An alias such as `B` or `reviewer` is a routing/display label, not durable agent identity. A pane is a layout presentation, not a session.

## Locked interaction decisions

These were open questions until 2026-08-31. They are recorded here because viewport and surface
code cannot be written without them, and changing either afterwards means a rewrite rather than
an adjustment.

### Screen ownership: full alternate screen

Plexmaton owns the alternate screen for its whole session and restores it on exit, including on
error and on panic. It does not render into inline scrollback.

Rationale: every locked surface behavior — floating inspectors, z-order, pointer capture,
independent viewports, hit testing against clipping rectangles — requires a coordinate space the
application controls completely. Inline scrollback gives the terminal ownership of scroll
position, which contradicts per-surface scroll ownership. The prototype already committed to this
in its terminal lifecycle; recording it makes the constraint reviewable rather than incidental.

Consequences that are now requirements, not options:

- Terminal-native selection is unavailable over owned regions, so the application-owned selection
  model and its documented modifier escape hatch are mandatory, not enhancements.
- Diagnostics and logs never write to the owned screen. They go to a file or an inspectable
  surface.
- Restoration must survive panic and signal paths, because a leaked alternate screen destroys the
  user's scrollback.

### Input: exactly one cursor

At any moment the workspace shows exactly one text cursor, and typed text can only reach it. Every
other rule about input follows from this one.

- The composer is bound to the **primary agent** and is never retargeted by selection. Its title
  names its target. Selecting, inspecting, or scrolling another agent does not change where typing
  goes.
- A sub-agent's steer input **does not render at all** unless that agent's surface holds keyboard
  focus. There is nothing to mistarget because there is nothing there.
- **Opening a sub-agent focuses it.** Opening one is an explicit user action, so its input appears
  immediately and is usable without a second step. This does not conflict with "background agents
  never steal focus": that rule constrains what agents do on their own, not what the user asks for.
  `Escape` closes the surface and returns focus to the primary conversation.
- When a sub-agent's input is active, the primary composer **collapses to a single row** reading
  `Message Agent A · ⇥ to return`. It does not disappear: a composer that vanishes costs the user
  the affordance and jumps the tail of the transcript they are reading. One row of jump is
  acceptable; three is not, and zero costs too much screen on a small terminal.
- The collapsed row stays clickable and stays a focus stop.
- A sub-agent's input takes its rows from its **own** surface budget — its content area shrinks and
  it scrolls slightly more. It may never consume the rows guaranteed to the primary conversation.
  Focusing a worker must never squeeze the primary conversation off the screen.

Rationale: the alternative — one composer whose target follows selection — is a mode-error
generator. The target is invisible state, and a misdirected instruction to a running worker is not
undone by sending another one. Making the input physically live inside the surface it addresses
turns "where does this keystroke go" into a fact on screen rather than something to remember.

Steering by explicit address (`@agent-b …`) from the primary composer remains available as a
keyboard path. It produces the same typed steering intent, with the target recorded in the message
itself, so it introduces no hidden state either.

### Nested scrolling: no propagation from an exhausted child

A wheel event routes to the topmost eligible viewport under the pointer and is consumed there.
When that viewport is already at its boundary, the event stops; it does not pass to the parent.

Rationale: this is the only policy consistent with the already-locked rule that a popup consumes
its own scroll without moving the transcript behind it. Propagation would make a viewport's
behavior depend on its scroll position, so the same gesture over the same pixel would sometimes
move a different surface — the exact spatial-memory failure this contract exists to prevent.

A viewport that cannot scroll at all is not eligible, so the event routes to the next eligible
viewport beneath it. "Exhausted" and "not scrollable" are deliberately different cases.

## Locked UX principles

### User control

- Background agents never steal keyboard focus, change the selected transcript, open a surface, or scroll a viewport automatically.
- New mail and state changes notify; they do not navigate on the user's behalf.
- Every mouse interaction has a discoverable keyboard equivalent.
- Destructive or high-impact actions identify their target agent/workspace before confirmation.

### Attention management

- Distinguish ambient activity, new information, action required, and failure. They must not share one generic notification treatment.
- Background progress is ambient. Completed mail is new information. Approval/clarification is action required. A failed or disconnected agent is failure.
- Background agents never open modal prompts directly over the user's active work.
- Action-required items enter a visible, ordered Attention queue. The user chooses when to focus the requesting agent unless an already-focused action blocks the current command.
- Repeated updates from one agent coalesce into one attention item instead of producing notification storms.
- Acknowledging a notification is distinct from resolving the underlying mail, approval, or failure.
- The Attention queue carries both directions of the user/agent relationship: an agent asking the user for something, and a delegating agent objecting to something the user changed. Neither may open a modal or take focus.

### Stable spatial memory

- Each surface preserves its own focus and scroll state while hidden, pinned, moved, or temporarily covered.
- Opening an inspector must not disturb the primary transcript's scroll anchor.
- Resize recomputes layout while preserving the semantic anchor in each visible transcript.
- Closing the top surface restores the previous focus predictably.

### Progressive disclosure

- Default views emphasize current conversation and agent state, not raw infrastructure events.
- Tool calls are compact by default but expose live status, result, and full output on demand.
- Mail presents sender, recipient, purpose, outcome, and durable pointers before transcript detail.
- Context/token/provider diagnostics are available without occupying permanent primary-screen space.

### Responsive interaction

- Input, scrolling, focus changes, and surface opening remain responsive while models stream and tools run.
- Streaming updates do not repeatedly re-layout content outside affected visible blocks.
- A background agent may update an ambient status indicator without forcing a full-screen redraw.
- Loading and failure states appear in the affected surface; they do not freeze unrelated surfaces.

### Readability

- Visual hierarchy comes from spacing, alignment, restrained color, and consistent component grammar before decorative borders.
- Agent, mail, tool, reasoning, artifact, warning, and error content are visually distinguishable without relying on color alone.
- Typeset math is the primary presentation; source is an interaction layer for inspect/copy and a clear failure representation.

### Selection and copy

- Mouse capture must not make transcript, tool output, paths, mail, or equations effectively uncopyable.
- Provide an application-owned selection model for semantic content and an explicit copy action with keyboard equivalents.
- Preserve a documented escape hatch for terminal-native selection where the terminal supports it, commonly through a modifier such as Shift.
- Copy transcript text from semantic source, not from border glyphs, clipped display cells, ANSI styling, or raster output.
- Copying a rendered equation returns its exact source. Copying an artifact/path returns the stable underlying value rather than a visually truncated label.
- Selection across virtualized content must either extend semantically beyond the current viewport or communicate a clear viewport boundary; it must not silently omit hidden text.

## Information architecture to validate

The experience skeleton must determine the durable arrangement of these product areas without assuming they are all permanently visible:

- Primary conversation and composer
- Agent navigator and agent lifecycle status
- Agent inspector: transcript, tool activity, mail, artifacts, and metadata
- Mail inbox/activity
- Tool output and diff inspection
- Session/context/provider diagnostics
- Command palette and help
- Permission, approval, and confirmation surfaces

The first prototype may place them provisionally. The exit gate requires evidence for what remains persistent, collapsible, overlaid, or command-driven.

## Surface model

Every interactive surface has:

- Stable surface identity
- Kind and ownership
- Rectangle and clipping rectangle
- z-order
- Visibility and modality
- Focusability
- Independent viewport/scroll state where applicable
- Drag/resize state for floating inspectors
- Minimum and preferred size
- Responsive fallback behavior

Surface categories:

| Category | Intended behavior |
| --- | --- |
| Base workspace | Primary layout; never floats above other surfaces |
| Peek inspector | Fast, non-destructive inspection; easy to dismiss or pin |
| Pinned pane | Participates in layout and persists while the user works elsewhere |
| Modal | Owns input until resolved or dismissed; background does not receive pointer events |
| Popover/menu | Anchored to an initiating element; closes on outside interaction or Escape |
| Tooltip | Informational only; never owns keyboard focus |
| Attention queue | Ordered action-required items; opening one is explicit and never caused by background focus theft |

### Shelf: overlay without occlusion

A peeked sub-agent renders as a **shelf** docked to the top edge of the conversation region, not as
a centred floating window.

The reason is that transcripts follow their tail, so the newest content sits at the bottom.
Covering the top hides what has already been read; covering the middle or bottom hides what the
user is reading now.

- Shelf height is `min(⌊0.55 × region⌋, region − 10)`, which guarantees at least ten rows of the
  primary conversation stay visible.
- The composer is never covered, at any size.
- When the region is shorter than 18 rows the guarantee cannot hold. The shelf then falls back to
  the maximized presentation rather than shrinking to a useless sliver.

Presentation and persistence are separate axes. **Pinned** is whether a surface survives the user
working elsewhere; **shelf, column, or maximized** is geometry chosen by terminal width. Changing
presentation must never change a surface's identity, scroll position, or focus.

### Drag scope

The only direct manipulation in the first slice is **dragging a shelf's bottom edge to change its
height**, clamped so the ten-row guarantee holds, with `Ctrl-Shift-↑/↓` as the keyboard equivalent.

Moving a panel freely around the terminal, like a desktop window, is deliberately **not** in the
first slice. A shelf is docked to the top of its conversation, so its position is determined and
only its height is a user choice. Free two-axis movement and eight-way resize would add pointer
capture on both axes, boundary clamping, resize recovery, and keyboard equivalents for each — for
a gesture nothing in the canonical journey needs yet.

This is a reduction from the first anatomy proposal, which asked for full floating-window
behaviour in Phase 00. Free drag returns when a pinned or maximized surface has a reason to be
somewhere other than where the layout puts it.

## Input and event-routing contract

- Pointer events route to the topmost visible surface whose clipped hit region contains the event coordinate.
- Wheel events use hover routing: the topmost eligible viewport under the mouse scrolls without changing keyboard focus.
- A consumed wheel event scrolls only its target viewport.
- Nested scrolling first targets the deepest eligible viewport. An exhausted child does not pass the event upward; see the locked decision above.
- Drag begins with pointer capture and continues to the captured surface until release or cancellation, even if the pointer leaves its rectangle.
- Keyboard input routes to the focused surface; hover alone does not redirect keyboard input.
- `Escape` resolves the topmost dismissible interaction before affecting the underlying workspace, one layer per press, and never quits.
- Modal surfaces block pointer and keyboard delivery to surfaces below them.
- Focus order and command availability must be inspectable for keyboard-only use.

The mechanism — translation, capture state machine, key grammar, and the numbered invariants — is
specified in [`specs/interaction-routing.md`](../specs/interaction-routing.md).

Exact click, double-click, context-menu, pin, maximize, drag, and resize bindings remain a Phase 00 design decision. They must be tested as one coherent grammar rather than assigned widget by widget.

## Multi-agent journey under test

The canonical Phase 00 journey is:

1. Agent A streams in the primary conversation.
2. A delegates a bounded task to agent B.
3. B appears as running without taking focus from A or the composer.
4. The user opens a peek inspector for B.
5. B's transcript and tool activity stream inside an independently scrollable viewport.
6. The user returns to A, continues typing, and optionally pins B for side-by-side observation.
7. The user freely moves/resizes B while A remains independently usable.
8. B requests approval or clarification; the request enters the Attention queue without opening a modal or stealing focus.
9. B sends typed mail to A; ambient status changes without automatic navigation.
10. The user opens the attention item/mail, follows an artifact, copies evidence, and can return to the exact prior viewport positions.
11. The user can steer, pause, abort, dismiss, reopen, or maximize B through explicit actions.

Every responsive layout must preserve the meaning of this journey even when it changes the placement of surfaces.

## Responsive layout classes

The exact thresholds are a prototype output, not a locked constant.

| Class | Product expectation | Threshold |
| --- | --- | --- |
| Ultrawide | Two conversations sit side by side; a second agent earns a column rather than an overlay | width ≥ 132 |
| Wide | Agent column plus one conversation; a second agent arrives as a shelf | 96 ≤ width < 132 |
| Medium | Primary conversation remains dominant; activity compresses to markers | 72 ≤ width < 96 |
| Narrow | One major surface at a time; agent switching and inspection become full-region transitions | width < 72 |
| Too small | One explicit notice, never a clipped workspace | width < 48 or height < 12 |

Implemented in `LayoutClass::for_size` and covered by tests, including that a too-small terminal
leaks no workspace content.

Ultrawide is 132 because two 52-cell conversations plus a 28-cell agent column need it, and 52
cells is roughly where prose stops wrapping awkwardly. Below it the shelf already handles a second
agent well, so there is no reason to force a cramped split.

At ultrawide there is **exactly one secondary column**, replaced when a different agent is selected.
Three live transcripts streaming at once is a monitoring product, not a working one.

Resize acceptance rules:

- Preserve focused semantic item when possible.
- Preserve bottom-follow only for viewports already following the tail.
- Preserve independent viewport anchors.
- Clamp inaccessible floating surfaces back into the visible area.
- Reflow content from source data; do not crop stale pre-resize strings.

## Transcript grammar to design

The prototype must establish reusable visual treatments for:

- User message
- Assistant streaming/final message
- Reasoning summary
- Tool call: queued, running, succeeded, failed, cancelled, approval required
- Diff and artifact
- Agent mail
- Delegation amendment: the user redirected a worker, shown in the delegator's transcript so the user and the delegating agent read the same story
- Agent objection: the delegating agent disputes an amendment, shown as action required rather than as a normal message
- Undelivered steering: a message that never reached its worker, with its original text intact
- System/runtime notice
- Warning and error
- Typeset display math and source reveal

The grammar must remain readable in monochrome and low-color terminals. Color enhances identity and status but is not the only carrier.

## State matrix

Each applicable surface needs an intentional representation for:

- Empty
- Loading
- Streaming
- Idle
- Waiting on tool/model/permission/descendant
- Paused
- Completed
- Failed
- Cancelled
- Disconnected/reconnecting
- Stale or unavailable persisted content
- Capability-degraded terminal
- Delegation amended by the user, delegating agent not yet informed
- Delegating agent objecting to an amendment
- Steering queued for a worker's next turn boundary
- Steering undeliverable, payload retained

Phase work should add states to this matrix when they become real; it should not defer all non-happy paths to product polish.

## UX performance budgets

Reproduce with `cargo run --release -p plexmaton-cli --bin plexmaton-measure`. The mechanism behind
these numbers is [`specs/frame-loop.md`](../specs/frame-loop.md); the split between what is asserted
and what is merely observed is FR-3 there, and it is the reason this table has two kinds of column.

Observed on an `arm64` macOS machine, release profile, 120 × 40, over a 5,000-message conversation —
the worst of the two scales the command runs. Targets are chosen against a 16 ms frame, so a
budget spent is a frame the user waits for.

| Budget | Target | Observed | Workload |
| --- | --- | --- | --- |
| Input event to visible frame | 5 ms | 0.97 ms p50, 1.2 ms max | `streaming delta` |
| Wheel event to visible scroll | 5 ms | 0.92 ms p50, 1.4 ms max | `wheel` |
| Streaming redraw frequency | one frame per changed projection, never per event | holds; ambient traffic that changes nothing costs no frame | FR-1 |
| Layout work per updated transcript block | 1 item wrapped | 1 wrapped, 27 lines built, at any history length | `streaming delta` |
| Opening an unmeasured conversation | 20 ms | 14 ms | `cold open`, and the first frame of `switch reader` |
| Resize recovery | 20 ms | 13 ms p50, 17 ms max | `resize` |
| Memory retained per hidden conversation | one cache entry per message | 5,000 entries for 5,000 messages; nothing else is retained | `switch reader` |
| Surface open/close latency | — | not yet measurable | Needs the shelf, delivery step 7 |

Two findings the numbers carry and a target alone would not:

- **Layout work is flat in history; total frame cost is not.** A steady frame wraps one item at any
  length, but still walks the item list five times to validate, sum, and locate. That is what puts
  a 5,000-message frame at ~1 ms against ~0.2 ms at 500. Linear in cheap operations, so the walks
  become the budget somewhere around 50,000 messages — which is where to look first, and not before.
- **Resize is the expensive interaction, because every height is width-dependent.** It re-measures
  each item exactly once, by design (TR-1), and that is 13 ms at 5,000 messages and would be 130 ms
  at 50,000. It is the first thing a retention limit or a lazily measured tail would be for.

## Phase 00 outputs

- Wide, medium, and narrow screen compositions
- Canonical A delegates to B interaction recording
- Surface/focus/event state diagram
- Initial keyboard and mouse grammar
- Transcript component grammar
- Design-token draft for spacing, color roles, borders, and elevation
- State-matrix examples for the canonical journey
- Attention hierarchy and queue behavior
- Transcript, path, artifact, and equation selection/copy behavior
- Measured responsiveness under synthetic streaming load

## Open design questions

- Persistent agent rail versus command-driven agent switcher at medium widths
- Floating-window placement, snapping, minimum sizes, and drag/resize bindings
- Pin/maximize interaction and keyboard bindings
- How much tool activity remains visible in collapsed transcript blocks
- Notification treatment for mail that arrives while its sender inspector is open
- Whether ten rows is the right primary-conversation guarantee under real transcripts
- Interaction between application selection, terminal-native selection, mouse capture, and tmux
- Whether selection may span virtualized off-screen transcript items in the first product slice
- Clipboard backend behavior across local desktop, SSH, tmux, OSC 52, and unavailable-clipboard environments

These questions should be resolved by the Phase 00 prototype and recorded here as durable interaction rules.

Five earlier questions have been answered and moved out of this list: the ultrawide threshold and
whether its second column is replaced on selection (D-024), the minimum supported terminal size
(D-025), low-colour behaviour (D-013), and whether the composer keeps `ratatui-textarea` (D-038 —
it is first-party, because the crate consumes terminal events and only one component may). Each is
stated in the section that owns it.
