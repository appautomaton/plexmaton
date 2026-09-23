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
observes delegated work, inspects evidence, stops work, and converses after handoff without losing
spatial context, scroll position, or control of the active conversation.

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
| Conversation surface | The interactive region displaying an agent's transcript; its activity line is conversation chrome |
| Turn | One admitted unit of work in a Conversation: what the user asked, everything the model and its tools did about it, and the answer that ended it |
| Step | One request to the model and the tool calls it comes back with. A turn is one or more steps, and a turn's budget is counted in them |
| Tool call | One invocation the model asked for, with a declared effect, a lifecycle, and bounded output |
| Mail | A typed, durable message delivered between Conversations |
| Controller | The sole actor currently allowed to send conversation input |
| Handoff | An explicit, durable transfer from main-agent control to user control |
| Artifact | Durable work product or evidence, referenced by identity or path rather than copied into mail |
| Surface | A rendered interactive region that participates in z-order and event routing |
| Viewport | The independently scrollable visible window over content owned by a surface |
| Inspector | The second window: one agent's conversation, shown over or beside the primary's while the user looks at that agent. `Inspector` is the code's name; user-facing copy names the agent |
| Peek | Looking at a sub-agent in the list, which opens the second window. The primary is not in the list, because its conversation is the screen |
| Command | A `/name` typed into a conversation's input and run from there, with its target captured. It means nothing else |
| Skill | A `$name` token bound into the message the input addresses |
| Composer menu | The popup above the composer: Skills for `$`, Commands for `/`. The draft is its query |
| Drawer | The workspace's own surface, pulled from the top edge by `Ctrl-P`. Its title, `Workspace`, is the addressee; it holds what outlives the process, as pages, never Commands |
| Page | A view inside the Drawer: Configuration, Permissions, opened in place |
| Server tool | A tool the provider runs on its own side inside one model call, declared per model in configuration. It is reported once it has ended; nothing here queues, admits or dispatches it |

An alias such as `B` or `reviewer` is a display label, never durable identity. A pane is a layout
presentation, not a Conversation. A widget is a Rust rendering component, not a synonym for an entry
or surface. `Agents` names the sub-agent list; it does not name the conversation surface.

## Locked interaction decisions

Viewport and surface code cannot be written without these, and changing one afterwards is a
rewrite rather than an adjustment.

### Conversation start: durable by default

Launching without a conversation argument prepares an automatically named durable Conversation. Its
JSONL is created on the first accepted user message; opening menus, editing a draft or exiting
without sending creates no file, and exit offers a resume command only for a saved one. Explicit
`create` reserves its file immediately, and only explicit `--ephemeral` declines persistence.
Rejected: an implicit ephemeral default, which makes an ordinary conversation vanish without the
user choosing that behavior.

### Delegated conversation control

The controller rule is [Roadmap §Locked](./roadmap.md#locked). A main-controlled child remains
inspectable and stoppable, with attributed mail and tool/artifact entries, but no direct user
composer or model-setting actions. Running and idle both retain the controller indication.

After acknowledged handoff, the composer becomes available without taking keyboard focus or
sending a message. History, selection/scroll anchors and the capability indication remain intact;
stop is independent of handoff.

This layout remains open to detail refinement. The user accepted its structure and function in the
[Kitty preview](./spikes/kitty-native-preview/README.md) on 2026-09-16; later polish must preserve
the control, focus and return behavior above.

### Context epochs and branch selection

Branches share earlier history and compact independently; a request uses the selected branch's own
compacted base and the turns after it. Every rewind creates and selects a new branch, leaving the
original head and its checkpoints untouched, and shares original entries without repeating tool
effects. Rejected: moving the original head during rewind, or using its latest checkpoint to
prohibit historical forks, because the original continuation and the target ancestry must remain
independent. CPL-5 proves checkpoint ancestry; [conversation tree](./specs/conversation-tree.md)
owns the rest.

### Screen ownership: full alternate screen

Plexmaton owns the alternate screen for its whole session and restores it on exit, including on
error and on panic. It does not render into inline scrollback. Rejected: inline scrollback, which
gives the terminal ownership of scroll position and contradicts per-surface scroll ownership, while
every locked surface behaviour, floating windows, z-order, pointer capture, independent viewports
and hit testing, needs a coordinate space the application controls completely.

Consequences that are requirements: terminal-native selection is unavailable over owned regions, so
the application-owned selection model and its modifier are mandatory; diagnostics and logs go to a
file or an inspectable surface, never the owned screen; and restoration survives panic and signal
paths, because a leaked alternate screen destroys the user's scrollback.

### Input: exactly one cursor

At any moment the workspace shows exactly one text cursor, and typed text can only reach it. Every
other rule about input follows from this one.

- The composer is bound to the **primary agent** and is never retargeted by selection. Its title
  names its target. Selecting, inspecting, or scrolling another agent does not change where typing
  goes. Rejected: one composer whose target follows the selection. The target is invisible state,
  and a misdirected instruction to a running worker is not undone by sending another.
- A sub-agent's input **does not render at all** unless its Controller is User and its surface holds
  keyboard focus. A Main-controlled surface remains inspectable and stoppable without an input
  region, whether the child is running or idle.
- **Entering a sub-agent's window focuses it; looking at one does not.** Selecting a sub-agent in
  the list opens its window and leaves the keyboard in the list, so the arrows keep moving through
  it. `Enter`, or a click in the window, moves the keyboard into its available controls. After an
  acknowledged Handoff, the same action also reveals and focuses its input; before Handoff there is
  no direct-input target. `Escape` closes the window, one layer of the routing ladder (INV-6).
  Rejected: focusing on look, which stops the arrows.
- **Waiting input remains visible beside its conversation**, in a band that never takes focus and
  yields to both composers. A message can be taken back into an empty draft, whole.
  [IQU-1–IQU-4](./specs/input-queue.md) own the band. Rejected: merging a returned message into an
  existing draft, which loses separate intent and can lose its skill binding.
- **Every rendered input sits under the conversation it addresses**, between two rules; optional
  waiting and decision sections sit above the composer. The conversation carries a box of its own,
  titled with whose it is, and the composer's top rule below it names the model that answers the
  next message, by the name its owner gave it, and the message's
  [reasoning effort](./specs/reasoning-effort.md), and carries nothing else — the box says whose
  conversation is being read, the rule says what will answer. Rejected: naming the agent, which
  repeated the box above it, because the primary composer can address no one else. A User-controlled sub-agent's input is the bottom of its window; a
  Main-controlled window has no input region, and there is no input anywhere else. The composer is
  not inside that box; it keeps its two rules.
- While a User-controlled sub-agent's input is active, the primary composer **collapses to a single
  row** reading `Message Agent A · ⇥ to return`, which stays clickable and stays a focus stop. Rejected: hiding
  it, which costs the affordance and jumps the tail of the transcript three rows; one row of jump is
  acceptable and zero costs too much screen on a small terminal.
- **The conversation ends in its activity line** while there is work or a request waiting, with one
  blank row between it and the composer: `Thinking`, `Responding`, `Running <tool>`, `Compacting`,
  or `Approval required`. It follows semantic state; action required outranks ambient work. A
  compaction says `Compacting` while it runs, whether the user asked for it or the runtime took it
  on its own. Idle with nothing
  waiting, it takes no rows, so the last reply sits one blank row from the composer; the rows open
  and close only with the conversation's own facts, never with the pointer. While the agent works
  the mark's core moves, and after the label the row says how long the turn has run, not counting
  its waits on the user, at what effort, and how long the agent's own route has been quiet. Show the
  approval label in the visible primary card; otherwise in the activity line. COM-5 owns the
  readings and the mark, [motion](./specs/motion.md) the clock. Rejected: duplicate labels on adjacent
  lines; a row kept blank while idle, which left a hole under the last reply; opening the rows for a
  selection note, which lifted the conversation under the reader's drag; a still row, which during a
  thirty-second silent turn read as a hang; a count that restarts with each step, which never
  shows the turn's total; a count that runs through an approval, which reports the user's decision
  as the next tool's run; a token count estimated from streamed characters,
  because the row claims only what the route reported; turning the row red when the route is quiet,
  because quiet is not failure.
- **An empty conversation shows the mark.** Before the first message the transcript holds the mark
  centred, the product's name beneath it, drawn in cells at the odd size nearest square for the
  terminal's measured cell; it moves on the same clock and leaves with the first message. Rejected:
  a fixed cell size, which is square in one font only.
- A User-controlled sub-agent's input takes its rows from its **own** surface. It may never consume
  the rows guaranteed to the primary conversation: focusing a worker never squeezes the primary off screen.
- **The composer completes the token it starts with.** `$` lists Skills and `/` lists Commands in
  the composer menu, above the input, without taking the caret; the draft is the query. Commands
  are what the user does inside a conversation, never workspace settings; a token no row matches is
  text. [`specs/composer-menu.md`](./specs/composer-menu.md) owns the roster and its keys.
  Rejected: workspace settings as slash commands, which made the composer's title lie about the addressee;
  and conversations as a Drawer page, which hid what users type by habit behind a chord.
- **The status line sits below every pane**, showing the working directory unless a configured
  script replaces it (STL-4). Rejected: a key-hint strip, a row of chords nobody read; overriding the first script row rather
  than the terminal's last; and the composer's bottom border, which belongs to one conversation,
  so a question from another agent's window was answered in the wrong box.
- **Quitting is a deliberate chord, and no single key leaves.** `Ctrl-C` clears a draft or
  interrupts, never exits. INV-7 owns the chord and its deadline. Rejected: an indefinitely armed
  chord, which lets unrelated later intent become an exit; and cancelling on an unrelated key,
  pointer event, or resize, which makes the time window depend on incidental input.

Placing the input directly under the conversation it addresses turns "where does this keystroke
go" into a fact on screen. Steering by explicit address, `@agent-b …`, is the same typed intent
with its target recorded in the message: no hidden state either.

### Nested scrolling: no propagation from an exhausted child

A wheel event is consumed by the viewport it routes to, and stops there even at that viewport's
boundary. A viewport that cannot scroll at all is a different case, and falls through. INV-3 owns
the routing. Rejected: propagation, which would make a viewport's behaviour depend on its scroll
position, so the same gesture over the same cell would sometimes move a different surface — the
exact spatial-memory failure this contract exists to prevent.

## Locked UX principles

### User control

- Background agents never steal keyboard focus, change the selected transcript, open a surface, or
  scroll a viewport.
- New mail and state changes notify; they do not navigate on the user's behalf.
- Every mouse interaction has a discoverable keyboard equivalent.
- Destructive or high-impact actions identify their target agent or workspace before confirmation.

### Attention management

A request is answered where the agent that raised it is; the roster says which agent that is.

The primary agent's approval opens in its own conversation and preserves the draft. Leaving grants
nothing; Deny starts selected. Allow and remember… reviews a backend-offered scope and lifetime
before granting. ATT-1/PER-5 own card behavior; inspection/copy follows
[APD](./specs/approval-inspection.md).

A background agent's request is announced in the roster, because the user is not in that
conversation to see it: the row takes the action-required color, sorts above the agents that are
only working, and says what is wanted. Requests never steal focus or open a modal; the user goes to
the agent, and acknowledging is not resolving. Repeated identities coalesce, and ambient progress,
new mail, action-required requests and failure stay distinct. Rejected: a separate Attention strip,
a third home for what the roster and the raising conversation already carry, charged to the
terminals with the fewest rows.

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
- A background agent updates ambient status without forcing a full-screen redraw.
- Loading and failure states appear in the affected surface and freeze nothing else.
- A resumed conversation is confirmed in place as UI only, with no transcript item or record, and
  an unfinished prior turn is told that nothing was rerun. JRN-5 owns the copy.
- **Notices** is the workspace's own voice: one bounded sentence no producer would say, costing the
  conversation no record — a message the runtime refused, or what a model switch cost. A refusal is
  Failure, a receipt Muted. Never what belongs in the transcript.
- An unanswered rate-limited request offers **Retry** and **Edit & retry** beside its error: they
  are message-local actions, never Commands, and neither repeats tools. JRN-8 owns eligibility and
  the bindings.
- Switching conversations waits for idle work and an empty draft. A working child does not block
  it: the first choice arms the last row with what the switch costs that child, the same choice
  again performs it, and its history is saved either way. SPK-1–SPK-4 own the rest.
- The terminal's last row is where the workspace asks for a gesture to be repeated — the quit chord,
  a switch that would stop a child — one question at a time, in Action required. Each states what
  repeating costs; doing something else instead withdraws it.

### Readability

- Visual hierarchy comes from colour, weight, spacing, alignment, and a consistent component
  grammar before decorative borders. Colour is the primary instrument, not a finishing coat: a
  workspace running several agents fills with concurrent activity, and colour is how the user
  finds the one row that needs them without reading the rest. Rejected: restrained colour with
  hierarchy carried by spacing alone, which made every row look equally important and left
  nothing to navigate by.
- Agent, mail, tool, server tool, reasoning, artifact, warning, and error content are
  distinguishable by colour, and carry a marker or name as well where that marker forms a scannable column.
  Rejected: requiring every distinction to survive without colour, which capped the design at
  what a colourless terminal could express.
- Explicit plaintext reasoning is named and visually quiet; system text is named and muted;
  warnings and errors carry action-required and failure colour and are named too, so a long
  transcript can be scanned for them. Opaque provider replay is never a visible transcript
  entry (PRV-3).
  Reasoning's trailing empty lines do not expand the gap before the next entry (ENT-1).
- A canonical diff keeps its source `+`/`-` markers and their colours, selected or not; unknown diff
  text remains undecorated source. [Transcript entry](./specs/transcript-entry.md) owns the roles.
- Typeset math is the primary presentation; source is an interaction layer for inspect and copy, and
  a clear failure representation.
- Workspace colour is eighteen semantic roles; widgets name a role, never a terminal colour, and
  a palette is a complete assignment of them. Colour says what a thing is and weight says what
  reads first; the row `Enter` acts on carries both (`Chosen`), and what stays quiet is a
  low-saturation hue with no weight on it. Document italic is a separate vocabulary with its own
  conventional uses (MD-5, MD-6).
  The status script owns its own colours (MD-5).
- **A palette is data, and colour is two layers.** Below, twelve slots named the way a terminal
  names them: a ground ramp — `ground`, `line`, `muted`, `text` — and eight hues around a closed
  wheel — `red`, `orange`, `yellow`, `green`, `cyan`, `blue`, `purple`, `magenta`. A theme assigns
  those twelve and nothing else. Above, the eighteen roles map onto slots and carry their weight;
  that mapping is the product's, so no theme can make a failure read as a success.
- A surface holding a conversation, or the roster of them, carries its own hue on its border:
  muted for the roster, blue for the user's own, cyan for a delegate's. Focus is that hue at full
  strength and rest is the same hue carried most of the way to the ground, so one border answers
  both questions a reader asks of it — whose surface this is, and where the keys are going. Surfaces that are something the
  workspace is saying rather than a place — a menu, a notice, an approval — keep the neutral line.
- **The terminal is assumed modern.** Plexmaton targets a 24-bit-colour terminal and a reader with
  ordinary colour vision. There is no reduced palette, no colour-capability probe, and no degraded
  path. The product already requires far more than colour depth — an animated effort rail on a
  67 ms clock (EFF-4), live repaint as agents stream, and native typeset math (MTH-2) — so a
  terminal that cannot render the palette was never going to run the product anyway. Carrying a
  fallback for it only capped what the first design could say. Rejected: an ANSI slot fallback and
  a modifier-only palette, which were a second design to keep correct for readers nobody had.

### Selection and copy

- Nothing the workspace draws is uncopyable: transcript, tool output, paths, mail and equations
  all reach the clipboard, and mouse capture never takes that away.
- **Copy is delivered to the clipboard at the user's terminal**, not the machine the process runs
  on, and reports what happened. Rejected: a local clipboard crate, which reaches the wrong machine
  over SSH.
- Pointer selection is a range of visible text; keyboard selection is a range of whole entries.
  Both survive scrolling and reflow. Copied text carries code indentation and semantic line breaks
  without chrome, Markdown syntax or soft-wrap newlines; the whole-message affordance copies the
  original source instead, Markdown included. A formula copies its whole original TeX.
- Hovering a message reveals its copy affordance without selecting, moving focus, covering text or
  reflowing. A foldable row toggles detail on click without selecting it. Rejected: automatic
  selection on disclosure, which obscures the detail the user opened.
- The terminal's own selection stays reachable through a modifier.
- Rejected: transcript selection reconstructed from terminal characters, which changes what is
  copied at a second width; and `Ctrl-C` as copy, which is the interrupt.

The mechanism, the bindings and the receipts are
[`specs/selection-and-copy.md`](./specs/selection-and-copy.md).

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

The inspector is the inspected agent's **conversation**. Tool activity and artifacts belong to
their producer. It shows both incoming and outgoing mail with sender/recipient attribution, as
projections of canonical items rather than copied transcripts. Mail status distinguishes queued
input from inclusion in a model turn. Entries retain first-appearance order; there is no separate
Activity surface. Each remaining area keeps its own interaction and presentation contract.

## Surface model

Every interactive surface has a stable identity, a kind and owner, a rectangle and a clipping
rectangle, a z-order, visibility and modality, focusability, its own viewport and scroll state
where applicable, drag and resize state where it floats, minimum and preferred sizes, and a
responsive fallback.

| Category | Behaviour |
| --- | --- |
| Base workspace | Primary layout; never floats above other surfaces |
| Second window | The sub-agent the user is looking at, floating over the primary's conversation or beside it at ultrawide; it stays while they work elsewhere |
| Modal | Owns input until resolved or dismissed; nothing beneath receives pointer events |
| Popover or menu | Anchored to an initiating element; closes on outside interaction or `Escape` |
| Tooltip | Informational only; never owns keyboard focus |
| Roster | The agents and what each is doing or waiting on; opening one is explicit and never caused by background focus theft |

The categories are closed: the Drawer is a modal, the composer menu a popover, and a feature adds
content to an existing surface, a Drawer page or a menu row, before it may add one. Rejected: one
surface per feature, which multiplied key routing, pressed-pointer state and fixtures per addition.

### Drawer: the workspace's own input

Docked to the top edge at full width, height from content. It floats over the rows already read
and may cover everything but the status line, because it blocks input anyway. It opens from any
focus state, even over a waiting approval, and never touches a draft: it addresses the workspace,
not a conversation. A page opens in place, and a handle on its bottom edge retracts the whole
Drawer from any page. DRW-1–DRW-3 own its pages, keys and chrome. Rejected: the command palette,
a centred overlay with three-cell margins, a dialog about nothing in particular.

### Shelf: overlay without occlusion

When the second window has room below ultrawide, it is a **shelf** docked to the top edge of the
conversation. It floats: the conversation beneath keeps its whole rectangle and its reading position, and
a conversation shorter than its panel sits at the bottom, so the shelf covers only empty rows or
rows already read. Transcripts follow their tail, so covering the top hides what has been read and
covering the middle or bottom hides what the user is reading now. Rejected: a centred floating
window, and splitting the region, which moved the conversation under it.

- The primary conversation keeps a readable minimum beneath the shelf and its composer is never
  covered; a region too short for both maximizes the window rather than drawing a sliver (INS-2).
- Which agent is shown is the selection. Shelf, column or maximized is geometry derived from
  terminal width and the user's maximize, and changing it changes no identity, scroll position or
  focus. There is no pin.

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
- Actual mouse movement within the focused menu selects its enabled row; arrows continue from that
  choice and Enter activates it. A stationary pointer never overrides keyboard navigation. Hover
  elsewhere accents enabled actions without moving keyboard focus, selecting transcript content or
  taking capture. Rejected: independent hover and keyboard choices in one menu, which showed two
  apparent answers to one pending action.
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
8. B requests approval or clarification; B's roster row says so without opening a modal or stealing
   focus.
9. B sends typed mail to A; ambient status changes without automatic navigation.
10. The user goes to the request, follows an artifact, copies evidence, and returns to the exact
    prior viewport positions.
11. While A controls B, the user can stop, dismiss, reopen or maximize B, but cannot send B input.
12. A explicitly hands B over after quiescence. The user may then converse with B directly;
    permissions and history remain unchanged.

Every layout class preserves the meaning of this journey even when it changes where surfaces go.

## Agents strip

The roster of sub-agents, above the user's own conversation.

- Absent until something is delegated: the conversation is the screen (INS-1).
- One agent, one row, and three rows at the most however many are running — so the conversation
  moves by the same amount whether two delegates are working or twenty. The rows go to the top of
  attention's order, which is already where what is addressed to the user sits; the title says how
  many exist. The selected agent keeps a row whatever its rank, because the row the next `Enter`
  acts on cannot be one the user cannot see.
- A row is three columns that line up down the strip: the name, bold, in the colour of the state it
  is in; that state, one word; then what the agent waits on, or a glyph tally of what has arrived
  for it, in one vocabulary every surface shares — a hammer and wrench for tool calls, a checklist
  for tasks, an envelope for letters *received*, a paperclip for artifacts, and on an inspected
  child a robot or a person for who is driving it beside a shield for the capability boundary every
  child has. An ask outranks a tally. The cursor row takes `Chosen`'s ground and keeps its own
  colour on top. Rejected: two rows an agent, which a 26-cell rail forced and which cost the strip
  its aligned columns; a `●` marker in the first cell, which spent the columns a name wants to say
  in a private glyph what `Chosen` says everywhere else; and a separate activity region, regrouping
  facts that belong in each agent's conversation.
- The strip is where a delegate's request is announced, because it is one row above what the user
  is already reading and attention's order puts the asking agent on the first of them. The pill on
  the conversation's border keeps the count (ATT-1), which is now the one fact the strip cannot
  carry: requests the cap left off, and the user's own. Rejected: a notification surface of its own,
  which would announce a third time what the row and the count already say.
- The strip belongs to the user's conversation and stops at its edge — with a delegate's window
  open beside it, it is the primary column's width; with a delegate maximized there is no primary
  for it to sit above and no strip. Rejected: spanning the terminal, which made the index of who is
  working *for the user* read as chrome over somebody else's transcript.
- Drawn: five agents in [one column](../crates/plexmaton-tui/frames/agents-strip-one-column.txt) and
  in [two](../crates/plexmaton-tui/frames/agents-strip-two-columns.txt), the
  [glyph vocabulary](../crates/plexmaton-tui/frames/agents-strip-tally.txt) once every request is
  answered, and [narrow](../crates/plexmaton-tui/frames/agents-strip-narrow.txt), where there is no
  strip at all.
- The strip can be put away and brought back, costing the conversation no column either way.
  Narrow has no strip: `Agents ^B` sits in the reserved conversation-top row, carrying `!n` only
  while a child is shown, and opens a full-region navigator over the same rows; leaving it restores
  exact prior focus. [Interaction routing](./specs/interaction-routing.md) owns the keys.
  Rejected: a shelf over a maximized child, a fixed column, and a band that made Agents yield first.

## Responsive layout classes

| Class | Product expectation | Threshold |
| --- | --- | --- |
| Ultrawide | Two conversations side by side; a second agent earns a column rather than an overlay | width ≥ 104 |
| Wide | One conversation; a second agent arrives as a shelf over it | 96 ≤ width < 104 |
| Medium | One conversation; a second agent arrives as a shelf over it | 72 ≤ width < 96 |
| Narrow | One major surface at a time; looking at an agent is a full-region transition, the window's maximized presentation | width < 72 |
| Too small | One explicit notice, never a clipped workspace | width < 48 or height < 12 |

- Wide and Medium compose identically and are two names for one class. They were distinguished by
  the width of the agent rail — 28 cells against 26 — and nothing else, so retiring the rail
  retired the difference. Named here rather than merged, because the merge is the user's to make.
- Nothing stands beside the conversation, so it is as wide as the terminal at every class. Ultrawide
  is two 52-cell conversations, which is roughly where prose stops wrapping awkwardly, and the
  threshold is exactly that. It holds one secondary column, replaced on selection. Rejected: a
  28-cell agent rail down the left, which charged every row of the conversation for a list that
  inked four percent of it and cut every ask it carried to a third of a sentence; and three live
  transcripts, which is a monitoring product rather than a working one.
- Below 48 × 12 the screen is one notice. Rejected: a clipped workspace.
- The Drawer keeps one geometry at every layout class: full width, height from content (DRW-2).
  Rejected: maximizing it on narrow screens, which filled the terminal with three rows, and a
  76-column centred overlay, which shrank abruptly as the terminal grew.
- Decision surfaces are spaced, not packed: clear cells inside their side borders, a blank row
  above and below, and a gap between data, choices and key hints. Short terminals drop optional
  spacing before controls or the composer.

Resize preserves the focused semantic item, keeps bottom-follow only for viewports already following
the tail, keeps every viewport's anchor, clamps an inaccessible floating surface back into view, and
reflows from source data rather than cropping stale strings.

## Transcript grammar

Each of these has one visual treatment. Colour carries identity and status; a marker or name
joins it where that marker lines up into a column the eye can run down, which is how a transcript
full of concurrent agents stays navigable:

- User message: its rows sit on a band of lifted ground across the conversation's width, opened by a
  `›` in blue, continuation indented under the text; nothing else marks it. Rejected: an
  accent-coloured bar down the turn's height, which shouted; folding long messages, because the
  transcript already owns long text.
- Assistant message, streaming and final
- Reasoning summary, named `reasoning` and visually quiet
- Tool call: `[ ] queued`, `[~] running`, `[?] approval required`, `[+] succeeded`, `[!] failed`,
  `[x] denied`, or `[-] cancelled`; retained invocation and outcome disclose beneath the same row
- Server tool call: the tool row's grammar, with the tool's name after the marker and what the
  route reported after the `·`. It appears `[~] running` where the provider began the call and
  finishes once: the query, `opened` and the page, or the pattern and the page it was sought `in`,
  and nothing further; a search reported without a query is the name alone. The marker wears the
  server-tool colour while running and when the call ended well, and failure's when it did not.
  What the route reported discloses beneath the row. The user chose the grammar and its colour
  from rendered candidates on 2026-09-21. Rejected: a row family of its own, which gave up the
  marker column and the disclosure tools already have; the plain succeeded colour, which read as
  a tool run inside the fence
- Diff with original `+`/`-` markers, and artifact
- Agent mail, and Main-authored task updates, each entering both conversations it names and saying
  in a word which side its row is: `sent to`, `received from`, `assigned to`, `assigned by`, naming
  the other end. The user read these on a real delegation and accepted the wording on 2026-09-14
- Handoff: an explicit change of controller, distinct from task completion or idle. Both named
  conversations receive one `handoff · Controller: User` row from the canonical durable fact
- Undelivered steering: a message that never reached its worker, with its original text intact
- System text, named and muted
- Context compacted: one system row where a checkpoint landed, `Context compacted: the conversation
  above continues from a summary.` A checkpoint the runtime takes on its own lands mid-turn,
  between the message that prompted it and the answer; one the user asks for lands after the last
  entry. It is there again when the conversation is reopened (CPL-4). The user chose it from
  rendered frames on 2026-09-23. Rejected: the transient note `/compact` used to leave, which a
  reopened conversation lost and an automatic checkpoint never had
- Warning and error, each carrying its colour and its name
- Typeset display math and source reveal

Waiting input is conversation chrome (IQU-1–IQU-4), not a transcript entry; it becomes history
only when its delivery boundary accepts it.

Related entries group tightly and unrelated ones are separated by one standard blank row; TR-6
owns the measured composition.

## State matrix

Each applicable surface has an intentional representation for: empty, loading, streaming, idle,
waiting on a tool, model, permission or descendant, paused, completed, failed, cancelled,
disconnected or reconnecting, stale or unavailable persisted content, a child controlled by Main while running or idle, handoff pending acknowledgement,
a user-controlled child ready for input, primary input waiting for delivery with an empty or
occupied draft, steering queued for a User-controlled worker's next step boundary, and input undeliverable with
its payload retained.

## Performance budgets

Targets are chosen against a 16 ms frame, so a budget spent is a frame the user waits for. Work
counts are asserted by tests; wall-clock time is only reported, beside the machine that produced it
(FR-4). The targets and the figures measured against them are in
[`specs/frame-loop.md`](./specs/frame-loop.md) §cost.

## Open questions

- How much tool activity remains visible in a collapsed transcript block.
- Notification treatment for mail that arrives while its sender's window is open.
- Whether ten rows is the right primary-conversation guarantee in real use.
- Where hosted-tool spend belongs in the cost surface: provider-side work the owner selected no
  model for, counted by the route in requests rather than tokens.

An answered question moves into the section that owns the answer.
