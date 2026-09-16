# Plexmaton Roadmap

| Field | Value |
| --- | --- |
| Product | A responsive, durable, multi-agent coding harness with a distinctive terminal interface |
| Open phases | Phase 04 |
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
| 02 | Canonical session state, persistence, durable permission policy, provider transports, context projection and project instructions | Closed 2026-09-12 by scoped evidence: delivered JSONL sessions and recovery, four provider dialects, compaction, durable permissions, AGENTS.md and conversation tree/rewind; unproven recovery/readmission acceptance inherited by Phase 03, independent export/import by Phase 04; MCP optional |
| 03 | Durable multi-agent mailbox and runtime ownership | Closed 2026-09-16: delivered ordered durable delegation/mail, owned child scheduling, Stop and Handoff control, passive and process-kill recovery, canonical Attention and the accepted responsive agent workspace |
| 04 | Product polish, performance hardening, math in production, and extensibility | Active; stages 1–9, 11–15 and 17–27 complete; stage 16 effort selection awaits user testing; stage 10 branding remains |

## Locked

Product invariants no phase may trade away, and no other document owns:

- Delegation is asynchronous, and mail between sessions is a typed atom rather than a turn, so the
  inspector can show it on its own and revisions reconcile against one log. A dialect with no native
  form for it renders mail as an attributed message naming its sender; an unattributed one, or a
  blocking tool result, is not delegation. Bulk findings stay where they were produced; mail carries
  a bounded summary and durable pointers.
- A delegated Conversation has one controller. While the main agent controls it, the user may
  inspect its attributed mail and stop work, but cannot send conversation input. An explicit,
  durable handoff after quiescence enables user input without expanding tool capabilities or
  replacing history; idle, completion and surface closure do not transfer control. Everything
  that moves between Conversations travels through one item log; inbox and Attention are
  projections over it.
- Mathematical content has one semantic source and one typeset layout. Raw LaTeX is never the
  routine presentation, on any terminal; the invariants are the math track's.
- User configuration and runtime state belong to one home the user owns, `~/.plexmaton/` by default
  and wherever `PLEXMATON_HOME` points otherwise, which is how a development profile stays out of
  the real one. Project configuration and skills must not redirect that ownership. `.agents/` is the shared project corpus; an optional project
  `.plexmaton/` holds client-specific settings and skills under [SKL-1–SKL-6](./specs/agent-skills.md).
  Claude Code remains the compatibility north star for external formats, through explicit adapters.

## Research

Tracks run beside phases because their exit conditions are comparisons, not delivered capability.

| Track | Purpose | Status |
| --- | --- | --- |
| [Math rendering](./research/math-rendering.md) | Evaluate semantic math layout and terminal presentation | Native RaTeX conversation path approved; portability and source-reveal gates open |
| [Multi-agent mailbox spike](./spikes/multi-agent-mailbox/README.md) | Compare local harness orchestration and test durable-mail semantics for Phase 03 | Source evidence retained; COL-1–COL-5 and CIN-1–CIN-4 verified locally and reachable from the binary |
| [Provider adapter parity spike](./spikes/provider-adapter-parity/README.md) | Compare harness wire/replay and token accounting | Six-source comparison; four dialects verified hermetically, live compatibility unverified |
| [Agent Skills spike](./spikes/agent-skills/README.md) | Compare skill discovery, project configuration and durable activation | Implemented and verified offline; live model behavior unverified |
| [Permission policy spike](./spikes/permission-policy/README.md) | Compare scoped grants, rule precedence and durable authority | Source comparison retained; PER-1–PER-10 and PGR-1–PGR-5 implemented and verified locally |
| [Compaction spike](./spikes/compaction/README.md) | Constrain compaction, branch-local context and recovery | Source/probe evidence retained; CPL-1–CPL-8 own implementation and regression evidence |

Gates without a track yet, each opened with a prototype, a comparison corpus, and a decision
criterion:

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
