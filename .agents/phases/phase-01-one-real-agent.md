# Phase 01 — One real agent

| Field | Value |
| --- | --- |
| Status | Active; opened 2026-09-02; step 1 next |
| Parent roadmap | [Plexmaton Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Depends on | The mechanisms and the layout Phase 00 delivered, each mechanism with a spec in [`specs/`](../specs/) |
| Unlocks | Phase 02 — session core, transports, tools, persistence |

## Outcome

A real model streams into the workspace and the user converses with it, and what it does with
tools is readable in the conversation as the contract describes. The composition is the contract's
at every width, with frames checked in. The semantic boundary is unchanged: the producer changed
from a scripted timeline to a model, and the projection did not notice.

## Inherited

The boundary is the code's own documentation. `plexmaton-core::PrototypeEvent` is what a runtime
emits and `plexmaton-tui::TuiIntent` is what the user emits; they never merge, nothing in the TUI
calls a runtime object (the `intent` module's doc says why), every variant survives a JSON round trip, and the tag is the wire
name. A producer numbers one monotonic sequence and advances each item's revision by exactly one;
the projection refuses a gap or a repeat.

| Event | Standing |
| --- | --- |
| `AgentCreated`, `AgentStatusChanged` | Durable |
| `TranscriptItemStarted`, `TranscriptDelta`, `TranscriptItemFinalized` | Durable; the cache and the anchors are built on this shape |
| `ToolActivityChanged` | Durable and thin: no arguments, no output, no expand state. Step 2 grows it |
| `AttentionRequested` | Durable, one way. Its resolution counterpart arrives with step 2 |
| `MailDelivered`, `ArtifactAnnounced` | Provisional: a bounded summary and a pointer, no body |
| `RuntimeWarning` | Durable; the degradation path |

Known limits carried in: `plexmaton-sim` is a scripted timeline with a one-verb command
vocabulary and is not a design for a runtime's command surface; the Attention queue has no
eviction; nothing removes a transcript item, so cache pruning has never run.

## Scope

1. **Composition, finished and frozen in frames.** One row per agent with its status on the row;
   the agent list a column from wide up and a band below; focus shown by the border, not the
   title; the status line on the last row; no user-facing word is `inspector`,
   `shelf` or `column`. Three frames of the canonical scenario, at wide, medium and narrow, checked
   in, compared by a test, and read by eye.
2. **The producer.** One provider adapter behind the semantic boundary: streaming, tool calls, a
   bounded tool set (read, search, run a command), the loop that executes a call and continues
   until the model stops asking, and approval before a mutating call. It emits `PrototypeEvent`,
   growing the vocabulary only where it must: a reasoning role, a typed tool detail (text or diff,
   bounded), an awaiting-approval tool state, a resolution for an attention item. The executable
   runs it; the simulator stays the test producer. The provider is chosen at the start of this
   step, and the choice is recorded beside the adapter's spec with what it rejected, once it has survived use.
3. **The transcript grammar, against real output.** Tool activity, mail and artifacts as entries
   in the owning agent's conversation, in arrival order, which retires the Activity panel and
   moves its counts to the agent row; a tool entry one compact row with a state marker, opening
   under the selection to its detail; a diff painting added and removed lines distinctly;
   reasoning, system, warning and error rows each with one treatment readable without colour.
   Entry heights keyed by revision, width and open state (TR-1 grows). Copy returns a tool's
   detail and a mail's summary.

Each step is sliced in a plan when it starts, and ends with frames at three widths looked at by
the user.

## Not in this phase

A second real agent, delegation, mail between sessions, the mailbox, pause and abort, and a composed
status-and-artifact surface beside a conversation: Phase 03. Persistence, context projection, MCP, a
second provider, and a session reducer beyond what the loop needs: Phase 02. Math rendering: its
track. Themes and animation: Phase 04.

## Exit gate

Assessed against the screen and the tests, per the corpus README.

| Criterion | Evidence expected |
| --- | --- |
| The user converses with a real model in the primary conversation and its text streams | A recorded fixture drives the adapter under `cargo test`; the executable runs it live |
| Every tool state, including failure and awaiting approval, is a compact conversation entry that opens to its detail | A test per state, and a frame per state read by eye |
| A diff entry paints added and removed distinctly and copies as its source | A test, and a frame |
| The three frames match the contract at wide, medium and narrow | Checked in, diffed by a test, read by the user |
| Streaming costs one wrap per delta at any history length, with a real producer | `FrameWork` asserted (FR-2) |
| The projection refused nothing under real traffic | The notice log is empty after a session; a test injects a gap and sees the refusal |
| No user-facing string says `inspector`, `shelf` or `column` | A test over the rendered strings |
| Nothing in the TUI calls the adapter or a tool | The crate graph: `plexmaton-tui` depends on `plexmaton-core` only |
