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
| Session | The ongoing coding period until the process exits; temporary permissions survive `/new` and `/resume` |
| Conversation | One agent's saved history; `/new` starts one and `/resume` reopens one |
| Project | A physical checkout whose personal permissions survive Sessions and application restarts |
| Journal | The authoritative Conversation record; JSONL is its on-disk encoding, not the visible transcript |
| Branch | A named continuation of a Conversation that shares earlier history with other branches |
| Context | Semantic input prepared for a model request; journal history combines with the applicable instructions and tools, excluding UI diagnostics |
| Context epoch | A branch's fixed context base and the later turns following it |
| Checkpoint | A durable compaction result that supplies the context base for its descendant heads |
| Transcript | The user-facing interaction history: messages, tool activity and diagnostics |
| Transcript entry | One identified content item in that history; a user or assistant message is a message entry |
| Conversation surface | The interactive region displaying an agent's transcript; its title and border are conversation chrome |
| Turn | One admitted unit of work in a Conversation: what the user asked, everything the model and its tools did about it, and the answer that ended it |
| Step | One request to the model and the tool calls it comes back with. A turn is one or more steps, and a turn's budget is counted in them |
| Tool call | One invocation the model asked for, with a declared effect, a lifecycle, and bounded output |
| Mail | A typed, durable message delivered between Conversations |
| Artifact | Durable work product or evidence, referenced by identity or path rather than copied into mail |
| Surface | A rendered interactive region that participates in z-order and event routing |
| Viewport | The independently scrollable visible window over content owned by a surface |
| Inspector | The second window: one agent's conversation, shown over or beside the primary's while the user looks at that agent. `Inspector` is the code's name; user-facing copy names the agent |
| Peek | Looking at a sub-agent in the list, which opens the second window; `Escape` closes it. The primary is not in the list, because its conversation is the screen |
| Command | A `/name` typed into a conversation's input and run from there, with its target captured. It means nothing else |
| Skill | A `$name` token bound into the message the input addresses |
| Composer menu | The popup above the composer: Skills for `$`, Commands for `/`. The draft is its query |
| Drawer | The workspace's own surface, pulled from the top edge by `Ctrl-P`. Its title, `Workspace`, is the addressee; it holds what outlives the process, as pages, never Commands |
| Page | A view inside the Drawer: Configuration, Permissions. Opens in place; `Escape` returns to the list |

An alias such as `B` or `reviewer` is a display label, never durable identity. A pane is a layout
presentation, not a Conversation. A widget is a Rust rendering component, not a synonym for an entry
or surface. `Agents` names the sub-agent list; it does not name the conversation surface.

## Locked interaction decisions

Viewport and surface code cannot be written without these, and changing one afterwards is a
rewrite rather than an adjustment.

### Conversation start: durable by default

Launching without a conversation argument prepares an automatically named durable Conversation. Its JSONL
is created on the first accepted user message; opening menus, editing a draft or exiting without
sending creates no file. Exit offers a resume command only for a selected saved Conversation. Explicit `create` reserves its file immediately.
Only explicit `--ephemeral` declines Conversation
persistence. Rejected: an implicit ephemeral default, which makes an ordinary conversation vanish
without the user choosing that behavior.

### Context epochs and branch selection

Branches share earlier history and compact independently. New model requests use the selected
branch's compacted base and subsequent turns, with the applicable instructions and tool definitions.
The checkpoint's persisted summary and retained context are reused without regenerating a summary.

Every rewind creates and selects a new branch at an eligible stable historical boundary. The
original branch, its head and its checkpoints remain unchanged. The new head uses the most recent
checkpoint on its own ancestry, followed by entries through the target; a path without a checkpoint
starts from its original context. Rewinding before a later checkpoint is therefore permitted:
that checkpoint does not belong to the new head's ancestry.

Selecting an existing branch restores its head and applies the same ancestry-based context rule.
Both operations share original entries without copying history or repeating tool effects. The
context epoch belongs to the target path; the source branch's latest checkpoint is not a rewind ban.

Rejected: moving the original head during rewind, or using its latest checkpoint to prohibit
historical forks, because the original continuation and the target ancestry must remain independent.
CPL-5 proves checkpoint ancestry. Branch and rewind interaction remains unproven;
[Phase 02 stage 2](./plans/phase-02-stage-02-context-projection.md) owns that journey and review.

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
- **Every input sits directly under the conversation it addresses**, between two rules; the
  conversation runs into the top rule with no edge of its own. The top rule names the target and
  the reasoning effort the message will get, and nothing else is written on either rule. A
  sub-agent's input is the bottom of its window. There is no input anywhere else, and `Tab` from
  a sub-agent's input lands on the primary composer, which is what the collapsed row's
  `⇥ to return` promises. Rejected: a box around conversation and input, chrome that said
  nothing; and current work on the composer's rule, mixing the agent's doing with the user's
  typing.
- While a sub-agent's input is active, the primary composer **collapses to a single row** reading
  `Message Agent A · ⇥ to return`, which stays clickable and stays a focus stop. Rejected: hiding
  it, which costs the affordance and jumps the tail of the transcript three rows; one row of jump is
  acceptable and zero costs too much screen on a small terminal.
- **The conversation's last row is its activity line**: `Thinking`, `Responding`,
  `Running <tool>`, or `Approval required`, directly above the composer's top rule. It is derived
  from semantic state, action required outranks ambient work, idle draws nothing, and it owns no
  animation clock.
- A sub-agent's input takes its rows from its **own** surface. It may never consume the rows
  guaranteed to the primary conversation: focusing a worker never squeezes the primary off screen.
- **The composer completes the token it starts with.** `$` lists Skills and `/` lists Commands in
  the composer menu, above the input, without taking the caret; the draft is the query. Commands
  are what the user does inside a conversation: `/new`, `/resume` over saved conversations,
  `/compact`, and `/permissions` for the Session. `Enter` accepts with the effect the row states;
  `Tab` completes without running; `Escape` keeps the draft. A token no row matches is text.
  Rejected: settings as slash commands, which made the composer's title lie about the addressee;
  and conversations as a Drawer page, which hid what users type by habit behind a chord.
- **The status line sits below every pane.** By default it shows the working directory; an explicitly
  configured user script may supply several styled rows, bounded to preserve typing and readable
  conversation space. The quit question temporarily replaces the terminal's last row and leaves
  the rows above it unchanged (STL-4).
  Rejected: a key-hint strip, a row of chords nobody read; overriding the first script row rather
  than the terminal's last; and the composer's bottom border, which belongs to one conversation,
  so a question from another agent's window was answered in the wrong box.
- **Quitting is `Ctrl-D` twice within one second.** The first press arms a monotonic one-second
  window and makes the status line say so; a second press before the deadline leaves, while expiry
  restores the underlying status content. Other terminal events neither confirm nor withdraw it. `Ctrl-C`
  withdraws it explicitly: with a non-empty draft it clears only the draft, and with an empty draft
  it interrupts only the focused conversation. It never quits. Rejected: an indefinitely armed
  chord, which lets unrelated later intent become an exit; and cancelling on an unrelated key,
  pointer event, or resize, which makes the time window depend on incidental input.

Placing the input directly under the conversation it addresses turns "where does this keystroke
go" into a fact on screen. Steering by explicit address, `@agent-b …`, is the same typed intent
with its target recorded in the message: no hidden state either.

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

The current runtime has one main agent. Its approval requests are handled inside its conversation,
never in an Attention bar, and answering one never touches a draft: leaving the card grants
nothing, and Deny starts selected. Allow and remember… reviews a backend-offered scope and
lifetime before granting anything. ATT-1 and PER-5 own arrival order, focus return, duplicate
submission and the card's anatomy.

Attention is reserved for future background-agent workflows, keeping ambient progress, new mail,
action-required requests and failure distinct. Background requests cannot steal focus or open
modals; the user visits them. Repeated identities coalesce, and acknowledging is not resolving.
It is never an entry point for main-agent approvals.

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
- A resumed conversation is confirmed in place, after its final entry, as UI only, with no
  transcript item or record. A genuinely unfinished prior turn also gets a warning that nothing
  was rerun; a failed or cancelled turn is finished and gets none. JRN-5 owns the copy.
- **Notices** is reserved for future multi-agent workflows, not routine operation feedback.
- An unanswered rate-limited request offers **Retry** and **Edit & retry** beside its error, by
  click or `r` / `e` with the primary transcript focused. They are message-local actions, never
  Commands. Retry continues the same path; Edit & retry fills the composer and preserves the old
  path, and `Esc` restores the displaced draft. Neither repeats tools; JRN-8 owns eligibility.
- `/resume` lists saved history behind the text after it; loading and errors stay inside the
  menu. Switching waits for idle work and an empty draft, never silently interrupting or
  discarding input; a new conversation creates storage on its first message unless ephemeral.
  SPK-1–SPK-3 own discovery and replacement.

### Readability

- Visual hierarchy comes from spacing, alignment, restrained colour, and a consistent component
  grammar before decorative borders.
- Agent, mail, tool, reasoning, artifact, warning, and error content are distinguishable without
  relying on colour alone.
- Explicit plaintext reasoning is named and visually quiet; system text is named and muted;
  warnings and errors are named before colour adds action-required or failure emphasis. Opaque
  provider replay is never a visible transcript entry (PRV-3).
- A canonical diff keeps its source `+`/`-` markers. Added lines use new-information, removed lines
  use failure, hunk headers use accent, and the patch envelope is muted. Selecting the entry adds the
  selection treatment without erasing those roles; unknown diff text remains undecorated source.
- Typeset math is the primary presentation; source is an interaction layer for inspect and copy, and
  a clear failure representation.
- Workspace colour is thirteen semantic roles; widgets name a role, never a terminal colour, and
  a palette is a complete assignment of them. The default is the status line's named colours:
  colour says what a thing is, weight what reads first, italic what stays quiet, and the row
  `Enter` acts on carries all three (`Chosen`). Without truecolor the chrome keeps its ANSI
  slots. The status script owns its own colours (MD-5).

### Selection and copy

- Mouse capture never makes transcript, tool output, paths, mail, or equations uncopyable.
- Hovering a message reveals a Copy icon at the right of its first row; clicking it copies the
  whole source without selecting it, moves no focus, covers no text and reflows nothing. A drag
  starts selection, and dragging off an action cancels it. SEL-7 owns the icon and the actions.
- Pointer selection is a grapheme-safe range of visible text across entries; releasing copies
  plain text with code indentation and semantic line breaks, without Markdown syntax, chrome or
  soft-wrap newlines. Keyboard selection is a range of whole entries. Both survive scrolling and
  reflow; a held drag beside the chrome scrolls into entries that began off-screen (SEL-6).
- A foldable tool row toggles retained detail on click without selecting or copying it; drag and
  keyboard gestures select source, and `Ctrl-O` toggles the selection's moving end. Hover changes
  nothing semantic (ENT-4). Rejected: automatic selection on disclosure, which obscures detail;
  and a nested viewport, an invisible second scroll owner.
- `Ctrl-Y` copies the current selection: plain text for pointer ranges, original source for
  keyboard entry ranges. The Copy icon always copies the whole message's original source,
  including Markdown. SEL-1/SEL-2 own mapping, table separators and streaming validation.
- Formula hits select and copy the whole original delimited TeX, even from blank-edge drags after
  clipping or reflow (MTH-1). Rejected: partial or bare-body copy, and clipboard-only expansion.
- The mouse reaches the terminal's own selection through a modifier escape hatch.
- Delivery goes to the clipboard at the user's terminal, not the machine the process runs on.
- Editable inputs take pointer placement and drag selection on grapheme boundaries; selected
  source is copied on release, and typing replaces it (COM-6).
- Rejected: transcript selection reconstructed from terminal characters, which changes what is
  copied at a second width; a local clipboard
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
- The Drawer: configuration, Project and User permissions
- Permission, approval, and confirmation surfaces

The inspector is the inspected agent's **conversation**. Tool activity, mail and artifacts are
entries in the conversation of the agent that produced them, in first-appearance order; there is
no separate Activity surface. A composed status-and-artifact surface is Phase 03's, and the other
areas are placed provisionally until the phase that builds them.

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

The categories are closed: the Drawer is a modal, the composer menu a popover, and a feature adds
content to an existing surface, a Drawer page or a menu row, before it may add one. Rejected: one
surface per feature, which multiplied key routing, pressed-pointer state and fixtures per addition.

### Drawer: the workspace's own input

Docked to the top edge at full width, height from content. It floats over the
strips and the rows already read, and may cover everything but the status line, because it
blocks input anyway. `Ctrl-P` opens it from any focus state, even over a waiting approval,
and never touches a draft: it addresses the workspace, not a conversation. A page opens in place,
and `Escape` returns one layer per press: page, list, then where it was opened from. Rejected: the
command palette, a centred overlay with three-cell margins, a dialog about nothing in particular.

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
- Bare pointer motion may change only a foldable row's visual accent; it never redirects input or
  takes pointer capture, and leaving the row clears it.
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
| Medium | A narrow agent column plus the primary conversation | 72 ≤ width < 96 |
| Narrow | One major surface at a time; looking at an agent is a full-region transition, the window's maximized presentation | width < 72 |
| Too small | One explicit notice, never a clipped workspace | width < 48 or height < 12 |

- The agent navigator is a column from medium up and a band below. Each row carries lifecycle plus
  compact non-text counts such as `1 tool @1 1 mail`; `@` is the artifact marker, while the
  conversation title keeps full nouns. Rejected: a separate activity region, which regrouped facts
  that already belong in each agent's conversation.
- Ultrawide is 132 because two 52-cell conversations and a 28-cell agent column need it, and 52
  cells is roughly where prose stops wrapping awkwardly. It holds exactly one secondary column,
  replaced on selection. Rejected: three live transcripts, which is a monitoring product rather than
  a working one.
- Below 48 × 12 the screen is one notice. Rejected: a clipped workspace.
- The Drawer keeps one geometry at every layout class: full width, height from content. Rejected:
  maximizing it on narrow screens, which filled the terminal with three rows; and a 76-column
  centred overlay, which shrank abruptly as the terminal grew.
- Approval cards and the Drawer leave two clear cells inside each side border, one blank
  row above and below their content, and a gap between data, choices and key hints. Short terminals
  drop optional spacing before controls or the composer; filtering keeps the input row stable.

Resize preserves the focused semantic item, keeps bottom-follow only for viewports already following
the tail, keeps every viewport's anchor, clamps an inaccessible floating surface back into view, and
reflows from source data rather than cropping stale strings.

## Transcript grammar

Each of these has one visual treatment, readable in monochrome and low-colour terminals; colour
carries identity and status but is never the only carrier:

- User message
- Assistant message, streaming and final
- Reasoning summary, named `reasoning` and visually quiet
- Tool call: `[ ] queued`, `[~] running`, `[?] approval required`, `[+] succeeded`, `[!] failed`,
  `[x] denied`, or `[-] cancelled`; retained invocation and outcome disclose beneath the same row
- Diff with original `+`/`-` markers, and artifact
- Agent mail
- Delegation amendment: the user redirected a worker, shown in the delegator's transcript so the
  user and the delegating agent read the same story
- Agent objection: the delegating agent disputes an amendment, shown as action required
- Undelivered steering: a message that never reached its worker, with its original text intact
- System text, named and muted
- Warning and error, each named before colour adds emphasis
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
| Compact tool entry to visible frame | 5 ms, and one entry wrapped | `compact tool entry` |
| Opening or closing one tool detail | 5 ms, and one entry wrapped | `open tool entry` |
| Wheel event to visible scroll | 5 ms | `wheel` |
| Surface open and close | 5 ms, and no re-wrapping | `open inspector` |
| Two conversations on screen, either scrolled | 5 ms, and no re-wrapping | `two conversations` |
| Extending a selection | 5 ms, and no re-wrapping | `extend selection` |
| Opening a conversation nothing has measured | 20 ms | `cold open`, `open hidden conversation` |
| Resize recovery | 20 ms | `resize` |
| Streaming redraw frequency | One frame per changed projection, never per event | FR-1 |
| Layout work per updated transcript block | One entry wrapped, at any history length | `streaming delta` |
| Memory retained per hidden conversation | One cache entry per transcript entry per width drawn, at most two widths | `open inspector` |

## Open questions

- The left agent rail and the top Attention strip: neither is the wanted shape yet.
- How much tool activity remains visible in a collapsed transcript block.
- Notification treatment for mail that arrives while its sender's window is open.
- Whether ten rows is the right primary-conversation guarantee in real use.

An answered question moves into the section that owns the answer.
