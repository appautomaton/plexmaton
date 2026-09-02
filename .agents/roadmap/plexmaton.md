# Plexmaton Roadmap

| Field | Value |
| --- | --- |
| Status | Working draft |
| Created | 2026-08-30 |
| Product | A responsive, durable, multi-agent coding harness with a distinctive terminal interface |
| Primary language | Rust |
| TUI foundation | Ratatui + Crossterm |
| Active phase | [Phase 01 — One real agent](./phase-01-one-real-agent.md), opened 2026-09-02 |
| UI/UX contract | [UI/UX](./ui-ux.md) |
| Mechanism specs | [specs/](../specs/) |

This document records product and architecture direction. It is intentionally not a feature checklist or an implementation promise. Decisions marked **Locked** are the current foundation; items marked **Research gate** require a focused prototype or measurement before selection.

## Roadmap navigation

Read this file first. For active work, continue only into the active phase file and the UI/UX sections it references. Future phases remain summaries here until the preceding phase reaches its exit gate.

Phases are named with two digits everywhere — `Phase 00`, not `Phase 0` — so prose, the table, and the file names stay greppable as one identifier.

| Phase | Purpose | Detail status |
| --- | --- | --- |
| 00 | Validate the experience and the event boundary with synthetic agents | Closed 2026-09-02 by scoping, not by a gate pass: it delivered the interaction mechanisms, each with a spec, and the contract's layout; the rest of the composition, the frames, the transcript grammar, and a real producer went to Phase 01 |
| 01 | One real agent in the workspace: a thin loop over one provider, and the transcript grammar against its output | [Active](./phase-01-one-real-agent.md) |
| 02 | Canonical session state, the remaining provider transports, tools, context projection, persistence, and MCP | Summary only; expected to split when opened |
| 03 | Durable multi-agent mailbox and runtime ownership | Summary only |
| 04 | Product polish, performance hardening, and extensibility | Summary only |

The cross-cutting [UI/UX contract](./ui-ux.md) applies to every phase. A phase may refine it with evidence, but may not silently invent conflicting interaction rules.

Research tracks run beside phases instead of inside them, because their exit conditions are
comparisons rather than delivered capability:

| Track | Purpose | Status |
| --- | --- | --- |
| [Math rendering](./track-math-rendering.md) | Select the math layout engine and both display transports | Not started; its entry condition is met |

## Product thesis

Plexmaton is a personal agentic harness whose internal state remains coherent while models stream, tools run, sessions branch, and background agents exchange work. Its Terminal User Interface (TUI) is not a chat box with decorations: it is an interactive workspace for observing and steering several agents without collapsing their transcripts into one noisy conversation.

The product should feel immediate under load, preserve completed work durably, and reveal complexity progressively. The user-facing agent remains usable while delegated agents work in the background.

## Locked foundations

### Runtime and TUI

- Use Rust for the harness and Ratatui + Crossterm for the TUI.
- Borrow Bubble Tea's unidirectional state-flow idea without adopting its Go runtime: events update explicit state and produce effects; rendering is a pure projection of state.
- Keep the TUI outside the authoritative agent/session state. It consumes revisioned snapshots and domain events.
- Network, model, tool, persistence, and math-render work must never block the terminal event loop.

### Interaction model

- Implement a small `SurfaceTree` / `WindowManager` owned by the TUI layer.
- Every pane, popup, inspector, and modal owns its own viewport and scroll state.
- Route mouse events using coordinates, clipping rectangles, and z-order. The topmost eligible surface at the pointer receives the event first.
- Support independent scrolling for overlapping surfaces. A popup consumes its own scroll events without moving the transcript behind it.
- Define focus, hover-scroll, drag capture, modal blocking, and nested scroll propagation explicitly rather than distributing ad hoc coordinate checks across widgets.
- Virtualize transcripts: layout and render only the visible range plus a small overscan region. Cache wrapping by content revision and viewport width.

### Multi-agent experience

- A delegated agent is a real session with its own identity, transcript, lifecycle, context, tools, and durable history.
- Delegation is asynchronous. Starting agent B returns control to agent A and the user-facing TUI immediately.
- Selecting an agent opens its conversation over or beside the primary's. Tool activity, mail and artifacts are entries in that conversation; a composed surface with status and an artifact index beside it is Phase 03's.
- Agent inspectors are independently scrollable and may be shown as a window floating over the primary conversation, a column beside it, or a maximized view without changing the underlying session.
- Agent-to-agent communication is typed mail between sessions, not a fake user message and not a blocking tool result.
- Bulk findings remain in artifacts or the delegated session; mail carries a bounded summary and durable pointers.

### Delegation has two writers and one record

A delegated task is the one place in this system where two parties write to the same thing: the
agent that created the task, and the user who can steer the worker directly. Treating either as
the owner produces divergence — the delegator reports the task it believes it assigned while the
worker does something else.

- A delegation is an append-only record owned by the runtime. The delegator's prompt and the
  user's steer are inputs to it, never parallel copies of the task.
- Every amendment is attributed and is delivered to the delegating agent before its next turn.
- The user's amendment wins on conflict, and the delegating agent may object but may not silently
  revert it.
- Everything that moves between sessions travels through one item log. The inbox and the Attention
  queue are projections over it, so they cannot drift apart.

This extends "one authoritative representation" from session state to delegation records.
Rejected: routing every steer through the delegator, a game of telephone that contradicts direct
steering; steering the delegator never sees, which makes its model of the task stale invisibly;
and separate inbox and attention stores with synchronization between them. The mechanism is
specified when Phase 03 opens.

## Readability-first math rendering

Mathematical content has one semantic source and one typeset layout. We do **not** treat raw LaTeX as the normal portable presentation and a rendered formula as an optional enhancement.

### Invariants

- Render recognized display math as KaTeX-quality typeset math whenever parsing succeeds.
- Choose the display transport according to terminal capability; do not change the equation's semantic/layout pipeline merely because one graphics protocol is unavailable.
- On terminals supporting Kitty, Sixel, or iTerm2 graphics, render the typeset result at pixel fidelity inside a cell-aligned region.
- On terminals without an image protocol, map the same rendered result into Unicode cell graphics such as Braille, half-block, or sextant glyphs with appropriate scaling and contrast. This is a lower-resolution transport, not a raw-source fallback.
- Preserve the original equation source behind every rendered formula.
- Clicking or selecting a formula reveals a stable source view without reflowing the surrounding transcript; copy actions return the exact original source.
- Keyboard-only users must have an equivalent reveal/copy action.
- Formula layout participates in normal viewport clipping and scrolling. Partially visible formulas must retain their logical image origin rather than restart rendering at the visible slice.
- Rendering is asynchronous and bounded. Cache by source, display width, theme, scale, renderer version, and terminal transport.
- If parsing or rendering genuinely fails, show an explicit readable error/source representation; do not silently display broken cell art.

### Research gate: math engine and transports

The invariants above are locked. Engine and transport selection is delegated to the
[math rendering track](./track-math-rendering.md), which owns the comparison corpus, the
candidates, and the decision criteria. Its entry condition is met: partial scrolling and source
copy, the properties that decide the engine, exist. The first thing it has to design is an item
kind that can report a provisional height and revise it, which is the shape a pending render has.

## Proposed system boundaries

```text
Terminal input
    -> TUI event loop
       -> SurfaceTree / focus / hit testing
       -> revisioned ViewState
       -> Ratatui renderer

Commands and user intents
    -> Runtime coordinator
       -> authoritative SessionState
       -> Agent/Turn state machines
       -> Context projector
       -> Tool scheduler and capability gates
       -> Provider adapters
       -> Mailbox supervisor
       -> durable event store
```

The runtime emits semantic events. The TUI decides how those events are presented. Provider wire events, durable session events, agent mail, and UI animation events must not be represented by one catch-all message type.

## Core representation direction

- Keep one authoritative session representation. An active turn may maintain transient assembly state, but must not own an independent full transcript that later needs reconciliation.
- Separate semantic conversation items from provider-specific replay metadata.
- Give every session item, tool call, tool result, mail item, turn, and agent a stable identifier.
- Derive the active provider context through a `ContextProjector` from session state, context policy, model capabilities, and a versioned tool-catalog snapshot.
- Implement Chat Completions, Responses, and Messages as independent wire adapters over the same semantic core and streaming event vocabulary.
- Preserve opaque provider continuation data under a provider/model-qualified envelope rather than overloading generic text or reasoning fields.
- Represent compaction, branching, retry, cancellation, and recovery as explicit state transitions and durable events.

## Network direction

- Start with one long-lived pooled HTTP client using Rustls through `reqwest` or `hyper`/`hyper-util`.
- Reuse DNS, TCP, TLS, and HTTP/2 connections where providers permit it.
- Separate connect/header timeout, stream-idle timeout, and whole-turn deadline.
- Apply backpressure between network decoding, semantic event assembly, persistence, and UI delivery.
- Coalesce high-frequency text deltas before TUI redraw while preserving exact transcript bytes in runtime state.
- Implement a bounded, chunk-safe Server-Sent Events parser with incomplete UTF-8 handling and provider-specific decoder state.
- Do not build a custom HTTP transport before profiling demonstrates a concrete limitation in the standard stack.

## Delivery phases

### Phase 00: experience skeleton

Closed 2026-09-02 by scoping rather than by its gate. It delivered the interaction mechanisms, each with a spec in [`specs/`](../specs/), and the contract's layout, against a scripted producer. The rest of the composition, the frames, the transcript grammar, and a real producer went to Phase 01.

### Phase 01: one real agent

Detailed plan: [Phase 01 — One real agent](./phase-01-one-real-agent.md).

- Finish the composition and freeze it in checked-in frames at three widths.
- Put one provider behind the semantic boundary: streaming, a bounded tool set, the loop, approval before a mutating call. The executable runs it; the simulator stays the test producer.
- Design the transcript grammar against that producer's real output, growing the event vocabulary only where it must.

### Phase 02: session core, transports, tools, and persistence

Expected to split when it opens; Phase 01's closure decides where.

- Define session, item, content block, tool, usage, provider replay, and streaming event types; implement the authoritative session reducer and active-turn state machine.
- Implement pooled HTTP transport and Server-Sent Events decoding; add the remaining provider adapters with recorded fixtures, and prove cross-adapter round trips without discarding provider continuation metadata.
- Add a versioned tool registry and capability-aware scheduler.
- Implement the initial file, search, edit, and command tools with bounded outputs.
- Select and implement the durable event store after measuring SQLite Write-Ahead Logging versus an append-only log plus index.
- Add context projection, token budgeting, compaction events, branching, retry, and crash recovery.
- Add native Model Context Protocol (MCP) discovery, trust, lifecycle, and tool mounting.

### Phase 03: durable multi-agent mailbox

- Add session-owned agents, parent/child lineage, hop lifecycle, cancellation, and resource budgets.
- Implement the one item log and its delivery states, with idempotent acknowledgement in the same storage boundary as session events.
- Implement the delegation record, its amendments, undeliverable steering, and the objection path.
- Keep the user-facing agent responsive while workers run.
- Add agent list, status indicators, mailbox activity, independently scrollable inspectors, artifact navigation, and explicit steering/abort controls.
- Begin with one mutating agent per workspace; make mutation ownership a runtime lease rather than a prompt convention.

### Phase 04: product polish and extensibility

- Stabilize the design system, themes, transitions, command palette, discoverability, accessibility, and degraded-terminal behavior.
- Productionize the selected math renderer and source interaction.
- Add performance budgets for input latency, redraw time, time to first token, memory, session reopen, and background-agent load.
- Design an out-of-process plugin protocol only after the native tool/MCP surface is stable. JavaScript compatibility is not an initial requirement.

## Explicit non-goals for the initial system

- Pi drop-in compatibility.
- Two complete TUI runtimes in one binary.
- A browser-style component framework or general desktop window manager.
- Unbounded transcript rendering.
- Blocking delegation disguised as a tool call.
- Agent identities defined only by aliases or prompt personas.
- Raw LaTeX as the routine display for terminals without image protocols.
- Embedded JavaScript/TypeScript extensions before the native runtime contracts stabilize.
- A custom HTTP/1.1 client without connection pooling.

## Open research gates

- Exact math layout engine and the minimum acceptable Unicode cell-graphics quality, owned by the [math rendering track](./track-math-rendering.md).
- Storage engine and transaction model for session events plus mailbox delivery.
- Initial terminal support matrix, especially tmux, SSH, Kitty graphics, Sixel, and terminals with no graphics protocol.
- Transcript layout cache structure and memory budget across many live agents.
- Plugin isolation model after MCP and native tools are stable.

## Decision rule

Prefer the smallest architecture that preserves the locked invariants. New subsystems require either a product capability that cannot be expressed cleanly in the current boundaries or measured evidence that the current implementation misses a declared latency, memory, correctness, or readability target.

## How this roadmap grows

- Expand one active phase at a time. Do not create detailed Phase 02–04 files merely to make the roadmap look complete.
- A phase file is created when its predecessor is approaching the exit gate and current evidence can constrain the next design.
- Put durable product and architecture invariants here; put cross-cutting interaction rules in `ui-ux.md`; put implementation scope and evidence in the active phase file.
- Promote a phase finding into this file only when it changes a durable invariant or system boundary.
- Record unresolved choices as research gates with an explicit prototype, comparison corpus, and decision criterion.
- Completion means the phase exit gate has evidence. File presence, code volume, or a successful happy-path demo is not completion by itself.
