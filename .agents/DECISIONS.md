# Decisions

An entry is a decision that was contested or is expensive to reverse: what was chosen, what it
rejected and why, and where the rule now lives. An uncontested choice is a commit, not an entry.
When a decision changes, its entry is rewritten; a replaced decision is named by its replacement
and has no entry of its own.

## Architecture

**D-001 · Rust, Ratatui and Crossterm; four crates, with `plexmaton-core` free of any terminal,
async or rendering crate; rendering is a pure projection of explicit state.** Rejected: a
runtime in the Bubble Tea mould; a component framework layered over Ratatui; and one crate,
which is how the reference project sprawled. Owner: `roadmap/plexmaton.md` §locked
foundations, `Cargo.toml`.

**D-003 · A producer contract violation degrades into a visible typed notice.** Rejected:
terminating the workspace, which hands the user a broken terminal for a bug they cannot see.
Owner: `plexmaton-tui::state::ViewState::apply`.

**D-029 · One router owns terminal-event translation, and declining an event is a named outcome.**
Rejected: per-widget event handling, and a silent fallthrough that makes a dead key
indistinguishable from a routing defect. Owner: [interaction-routing](./specs/interaction-routing.md) INV-1.

**D-030 · Intents live in `plexmaton-tui`; `plexmaton-core` is the runtime-to-projection boundary.**
Rejected: `TuiIntent` in core. Scroll, focus and pointer capture are no runtime's business; a user
action that must reach one becomes a core command at the composition boundary. Owner:
`plexmaton-tui::intent`.

**D-036 · Surface identities are named; the renderer returns the registry it drew and routing
hit-tests only that.** Rejected: numeric identities, which break silently when a region is added;
and a second layout computed for hit testing, whose failure is a click landing one panel over.
Owner: [surface-model](./specs/surface-model.md) SURF-1.

**D-041 · One `Workspace` drives the executable and the measurement harness; frame work is
asserted, frame time only reported.** Rejected: `criterion`, which cannot see work counts; and
wall-clock assertions, which the same binary on the same laptop moved by a factor of two hours
apart. Owner: [frame-loop](./specs/frame-loop.md) FR-3.

**D-019 · A delegation is one record with two writers; the user's steer is an amendment the
delegator sees before its next turn.** Rejected: routing every steer through the delegator, a game
of telephone that contradicts direct steering; and steering the delegator never sees, which makes
its model of the task stale invisibly. Owner: [delegation-and-steering](./specs/delegation-and-steering.md).

**D-020 · Inbox and Attention queue are projections over one item log.** Rejected: separate
stores with synchronization between them. Owner: [mailbox-delivery](./specs/mailbox-delivery.md) INV-1, INV-7.

## Screen

**D-005 · Plexmaton owns the full alternate screen.** Rejected: rendering into inline scrollback,
which gives the terminal ownership of scroll position and contradicts per-surface scroll
ownership. Owner: `roadmap/ui-ux.md` §screen ownership.

**D-013 · Colour is twelve semantic roles; a palette is any complete assignment of them, the
three constructors are presets, and `ansi` is the default so the user's terminal theme wins.**
Rejected: terminal colours named inside widgets, and the presets as a closed set, which makes a
new colourway a fourth constructor. Owner: `plexmaton-tui::theme`.

**D-014 · The agent column sits on the left and holds the sub-agent list over the activity in one
box.** Rejected: an activity column on the right, stacked under the conversation at medium, which
left the screen whenever a second window opened. Owner: `roadmap/ui-ux.md` §responsive layout classes.

**D-024 · Ultrawide starts at 132 and holds exactly one secondary column, replaced on selection.**
Rejected: three live transcripts, which is a monitoring product rather than a working one. Owner:
`roadmap/ui-ux.md` §responsive layout classes.

**D-016 · The second window is a shelf docked to the top of the conversation, floating over it.**
Rejected: a centred floating window, which covers what the user is reading; and splitting the
region, which shipped once and moved the conversation under it. Owner: `roadmap/ui-ux.md` §shelf.

**D-028 · Direct manipulation is the shelf's vertical resize only.** Rejected: free two-axis drag
and eight-way resize now, for a gesture nothing in the journey needs. Owner:
`roadmap/ui-ux.md` §drag scope.

**D-049 · The second window is the selection: the list holds only sub-agents, selecting one floats
its conversation over the primary's, and `Escape` closes it.** Replaces D-042, which stored the
open window beside the selection with pin and follow; the default path put one conversation on
screen twice, and deriving the window from the selection makes that unrepresentable. Owner:
[inspector](./specs/inspector.md) INS-1.

**D-046 · Until Phase 03 the second window is a conversation and nothing else.** Rejected: composing
tools, mail, artifacts and status into it now, which needs sub-region scroll ownership and an
expand model the transcript lacks; Phase 03 owns it. Owner: [inspector](./specs/inspector.md) INS-6.

## Input

**D-017 · The composer is bound to the primary agent and is never retargeted by selection.**
Rejected: one composer whose target follows the selection; the target is invisible state, and a
misdirected steer to a running worker is not undone by sending another. Owner: `roadmap/ui-ux.md` §input.

**D-018 · Exactly one cursor exists; a sub-agent's input renders only while its surface holds
focus.** Rejected: a visible unfocused input, which is something to mistarget. Owner:
`roadmap/ui-ux.md` §input.

**D-026 · Entering a sub-agent's window focuses it; looking at one does not.** Rejected: focusing
on look, which stops the arrows moving through the list. Owner: [inspector](./specs/inspector.md) INS-4.

**D-027 · While a sub-agent's input is active the primary composer collapses to one row.**
Rejected: hiding it, which costs the affordance and jumps the transcript tail three rows;
performance was never the reason. Owner: `roadmap/ui-ux.md` §input.

**D-031 · `Escape` resolves one layer per press and never quits; `Ctrl-C` is the only quit.**
Rejected: `Escape` as quit, then a bare `q`. Both shipped and both ended sessions by reflex,
`Escape` once a dismissible layer existed and `q` because focus starts on a navigation surface.
Owner: [interaction-routing](./specs/interaction-routing.md) INV-6, INV-7.

**D-038 · The composer is first-party, and a submitted message is a runtime command rather than a
write.** Rejected: `ratatui-textarea`, which consumes terminal events when only the router may;
and the projection appending its own transcript, which puts two writers on one numbered stream.
Owner: [composer](./specs/composer.md) COM-3.

**D-043 · A selection is a range over a surface's entries; copy is `Ctrl-Y`, delivered by OSC 52.**
Rejected: character selection, which changes what is copied at a second width; `arboard`, which
reaches the wrong machine over SSH; and `Ctrl-C` as copy, which is the exit. Owner:
[selection-and-copy](./specs/selection-and-copy.md).

**D-006 · An exhausted child viewport does not pass the wheel to its parent.** Rejected:
propagation, which makes the same gesture over the same cell move a different surface depending
on scroll position. Owner: `roadmap/ui-ux.md` §nested scrolling.

## Transcript

**D-039 · A viewport measures its content through the same `Paragraph` that paints it.** Rejected:
owning the wrapping, roughly eighty lines reaching the same answer with our own bugs; the exact
pin and the lockfile make an unstable-API change a reviewed bump. Owner:
[surface-model](./specs/surface-model.md) §viewports.

**D-040 · Heights are cached per item by revision and width; a reader is parked on a message, per
conversation.** Rejected: a row offset, which survives a resize as a number while naming different
text; a per-surface position, which loses A's place on returning from B; and bounded overscan,
which a synchronous renderer cannot use. Owner: [transcript-layout](./specs/transcript-layout.md) TR-1, TR-3, TR-5.
