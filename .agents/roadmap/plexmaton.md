# Plexmaton Roadmap

| Field | Value |
| --- | --- |
| Status | Working draft |
| Created | 2026-08-30 |
| Product | A responsive, durable, multi-agent coding harness with a distinctive terminal interface |
| Primary language | Rust |
| TUI foundation | Ratatui + Crossterm |
| Active phase | [Phase 00](./phase-00-experience-skeleton.md), reopened on 2026-09-01 for one experience-alignment step after passing its exit gate on 2026-08-31; Phase 01 — Session and Provider Core — follows |
| UI/UX contract | [UI/UX](./ui-ux.md) |
| Decision index | [DECISIONS.md](../DECISIONS.md) |
| Mechanism specs | [specs/](../specs/README.md) |

This document records product and architecture direction. It is intentionally not a feature checklist or an implementation promise. Decisions marked **Locked** are the current foundation; items marked **Research gate** require a focused prototype or measurement before selection.

## Roadmap navigation

Read this file first. For active work, continue only into the active phase file and the UI/UX sections it references. Future phases remain summaries here until the preceding phase reaches its exit gate.

Phases are named with two digits everywhere — `Phase 00`, not `Phase 0` — so prose, the table, and the file names stay greppable as one identifier.

| Phase | Purpose | Detail status |
| --- | --- | --- |
| 00 | Validate the experience and architectural event boundaries with synthetic agents | [Gate passed; step 09 in progress](./phase-00-experience-skeleton.md); the handoff is its last section |
| 01 | Canonical session state and the three provider transports | Summary only |
| 02 | Tools, context projection, persistence, and MCP | Summary only |
| 03 | Durable multi-agent mailbox and runtime ownership | Summary only |
| 04 | Product polish, performance hardening, and extensibility | Summary only |

The cross-cutting [UI/UX contract](./ui-ux.md) applies to every phase. A phase may refine it with evidence, but may not silently invent conflicting interaction rules.

Research tracks run beside phases instead of inside them, because their exit conditions are
comparisons rather than delivered capability:

| Track | Purpose | Status |
| --- | --- | --- |
| [Math rendering](./track-math-rendering.md) | Select the math layout engine and both display transports | Not started; **unblocked** on 2026-08-31 by the Phase 00 viewport |

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
- Selecting an agent opens an inspector surface showing its transcript, tool activity, mailbox traffic, status, and artifacts. The composed surface is Phase 03's; Phase 00's inspector is the conversation, and the other four belong to the activity column (D-046).
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
Mechanism detail: [delegation and steering](../specs/delegation-and-steering.md) and
[mailbox delivery](../specs/mailbox-delivery.md).

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
candidates, and the decision criteria. It was blocked on the Phase 00 viewport, because partial
scrolling and source copy are the properties that actually decide the engine; both now exist, and
what the track has to design first is an item kind that can report a provisional height and revise
it, which is the shape a pending render has and nothing in Phase 00 needed.

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

Detailed plan: [Phase 00 — Experience Skeleton](./phase-00-experience-skeleton.md).

- Establish only the workspace and event boundaries needed by the experience prototype.
- Run synthetic streaming agents through the same semantic UI boundary intended for the real runtime.
- Validate `SurfaceTree`, independent viewports, multi-agent inspection, and responsive layout before provider and persistence complexity arrives.
- Add deterministic TUI snapshots and interaction tests before visual complexity grows.

### Phase 01: canonical session and provider transport

- Define session, item, content block, tool, usage, provider replay, and streaming event types.
- Implement the authoritative session reducer and active-turn state machine.
- Implement pooled HTTP transport and Server-Sent Events decoding.
- Add Chat Completions, Responses, and Messages adapters with recorded fixtures.
- Prove cross-adapter round trips without discarding provider continuation metadata.

### Phase 02: tools, persistence, and context

- Add a versioned tool registry and capability-aware scheduler.
- Implement the initial file, search, edit, and command tools with bounded outputs.
- Select and implement the durable event store after measuring SQLite Write-Ahead Logging versus an append-only log plus index.
- Add context projection, token budgeting, compaction events, branching, retry, and crash recovery.
- Add native Model Context Protocol (MCP) discovery, trust, lifecycle, and tool mounting.

### Phase 03: durable multi-agent mailbox

- Add session-owned agents, parent/child lineage, hop lifecycle, cancellation, and resource budgets.
- Implement the one item log and its delivery states, with idempotent acknowledgement in the same storage boundary as session events. See [mailbox delivery](../specs/mailbox-delivery.md).
- Implement the delegation record, its amendments, undeliverable steering, and the objection path. See [delegation and steering](../specs/delegation-and-steering.md).
- Keep the user-facing agent responsive while workers run.
- Add agent list, status indicators, mailbox activity, independently scrollable inspectors, artifact navigation, and explicit steering/abort controls.
- Begin with one mutating agent per workspace; make mutation ownership a runtime lease rather than a prompt convention.

This phase grew on 2026-08-31. The delegation record, amendment events, undeliverable payloads, and
the objection flow were not in the original summary; they arrived from the two-writers finding
above. Recording the growth now is cheaper than meeting it as a surprise when the phase opens.

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

- Expand one active phase at a time. Do not create detailed Phase 01–04 files merely to make the roadmap look complete.
- A phase file is created when its predecessor is approaching the exit gate and current evidence can constrain the next design.
- Put durable product and architecture invariants here; put cross-cutting interaction rules in `ui-ux.md`; put implementation scope and evidence in the active phase file.
- Promote a phase finding into this file only when it changes a durable invariant or system boundary.
- Record unresolved choices as research gates with an explicit prototype, comparison corpus, and decision criterion.
- Completion means the phase exit gate has evidence. File presence, code volume, or a successful happy-path demo is not completion by itself.
