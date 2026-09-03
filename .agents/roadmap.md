# Plexmaton Roadmap

| Field | Value |
| --- | --- |
| Product | A responsive, durable, multi-agent coding harness with a distinctive terminal interface |
| Open phases | None; Phase 02 is not opened |
| UI/UX contract | [UI/UX](./ui-ux.md) |
| Mechanism specs | [specs/](./specs/) |

Direction, not a schedule: why the product exists, the order it is built in, what it will not be,
and what is still undecided. What exists today is the active phase's business; the experience the
phases build toward is the contract's; how this file grows is the corpus README's.

## Thesis

Plexmaton is a personal agentic harness whose internal state remains coherent while models stream,
tools run, sessions branch, and background agents exchange work. Its Terminal User Interface (TUI)
is not a chat box with decorations: it is an interactive workspace for observing and steering
several agents without collapsing their transcripts into one noisy conversation.

The product should feel immediate under load, preserve completed work durably, and reveal
complexity progressively. The user-facing agent remains usable while delegated agents work in the
background.

## Phases

A phase is open while its file exists in `phases/`, and more than one may be, because work is
cross-cutting. The contract applies to all of them, and a phase refines it only as the write-routing
table in `AGENTS.md` allows.

| Phase | Purpose | Status |
| --- | --- | --- |
| 00 | Validate the experience and the event boundary with synthetic agents | Closed 2026-09-02 by scoping, not by a gate pass: it delivered the interaction mechanisms, each with a spec, and the contract's layout; the rest of the composition, the frames, the transcript grammar, and a real producer went to Phase 01 |
| 01 | One real agent in the workspace: a thin loop over one provider, and the transcript grammar against its output | Closed 2026-09-03: delivered one live OpenAI-compatible agent, five bounded native tools with explicit approval and cancellation, and the reviewed responsive transcript grammar |
| 02 | Canonical session state, persistence, durable permission policy, the remaining provider transports, context projection, and MCP | Not opened; expected to split when it is |
| 03 | Durable multi-agent mailbox and runtime ownership | Not opened |
| 04 | Product polish, performance hardening, math in production, and extensibility | Not opened |

## Locked

Product invariants no phase may trade away, and no other document owns:

- Delegation is asynchronous, and agent-to-agent communication is typed mail between sessions:
  never a synthesized user message, never a blocking tool result. Bulk findings stay in artifacts
  or the delegated session; mail carries a bounded summary and durable pointers.
- A delegation is one record owned by the runtime with two writers, the delegating agent and the
  user. Every amendment is attributed and reaches the delegator before its next turn; the user's
  wins on conflict, and the delegator may object but not silently revert. Everything that moves
  between sessions travels through one item log, and the inbox and the Attention queue are
  projections over it.
- Mathematical content has one semantic source and one typeset layout. Raw LaTeX is never the
  routine presentation, on any terminal; the invariants are the math track's.
- Project repositories never need or implicitly load a `.plexmaton/` directory. `.agents/` is the
  project-owned instruction and skill corpus; `~/.plexmaton/` is user-owned configuration and
  runtime state. Claude Code is the compatibility north star for external project/skill formats,
  consumed through explicit adapters without inheriting its internal ownership or implicit trust.

## Research

Tracks run beside phases because their exit conditions are comparisons, not delivered capability.

| Track | Purpose | Status |
| --- | --- | --- |
| [Math rendering](./research/math-rendering.md) | Select the math layout engine and both display transports | Not started; its entry condition is met |

Gates without a track yet, each opened with a prototype, a comparison corpus, and a decision
criterion:

- The storage engine and transaction model for session events plus mailbox delivery: SQLite
  Write-Ahead Logging against an append-only log plus index.
- The terminal support matrix: tmux, SSH, Kitty graphics, Sixel, and terminals with no graphics
  protocol.
- The transcript layout cache structure and memory budget across many live agents.
- The plugin isolation model, after MCP and native tools are stable.

## Non-goals

- Pi drop-in compatibility.
- Two complete TUI runtimes in one binary.
- A browser-style component framework or general desktop window manager.
- Unbounded transcript rendering.
- Agent identities defined only by aliases or prompt personas.
- Embedded JavaScript/TypeScript extensions before the native runtime contracts stabilize.
- A custom HTTP/1.1 client without connection pooling.
