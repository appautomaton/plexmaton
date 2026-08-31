# AGENTS.md — Plexmaton

Plexmaton is a Rust agentic harness with a responsive Ratatui interface, explicit session/context state, multiple provider dialects, durable tools and sessions, and asynchronous multi-agent collaboration. It is early-stage: preserve the design intent, but do not manufacture compatibility or abstraction for users and APIs that do not yet exist.

This file inherits the workspace rules in `../AGENTS.md`; the stricter applicable rule wins. It is the only project document loaded on every turn, so it holds what shapes behaviour before you know the task. Everything else loads on a trigger. [`.agents/README.md`](./.agents/README.md) explains the layering and owns the corpus rules.

## Context routing

Read only the material the active task needs. Progressive disclosure is an enforced working rule, not a documentation style: do not load every roadmap phase, all reference repositories, or broad source trees by default.

| When you are about to… | Read |
| --- | --- |
| decide something that feels already settled | [`.agents/DECISIONS.md`](./.agents/DECISIONS.md) — an index; follow its links |
| start or resume work on this phase | the active phase file named by [`roadmap/plexmaton.md`](./.agents/roadmap/plexmaton.md) |
| implement or review a named mechanism | [`.agents/specs/<mechanism>.md`](./.agents/specs/README.md) — cite its invariant in the test that proves it |
| start a step big enough to have an order | [`.agents/plans/`](./.agents/plans/README.md) — slice it before writing code; delete the plan when consumed |
| change interaction, layout, focus, attention, or copy behaviour | the relevant sections of [`roadmap/ui-ux.md`](./.agents/roadmap/ui-ux.md) |
| write, change, or delete a test | [`standards/testing.md`](./.agents/standards/testing.md) |
| organize a module, or add/upgrade/remove a dependency | [`standards/rust.md`](./.agents/standards/rust.md) |
| run a gate, fix a failing one, or set up a clone or parallel checkout | [`standards/quality-gates.md`](./.agents/standards/quality-gates.md) |
| write or reorganize a document | [`.agents/README.md`](./.agents/README.md) |
| compare against a third-party implementation | `.references/`, which is gitignored and absent in a fresh clone. Treat its absence as normal |

If `.agents/handoffs/` contains a letter, it is an orientation note a previous session was asked to leave. Reading the newest one is optional and cheap when picking up unfamiliar work; it never holds a rule.

**Cite, don't restate.** Reference `D-027`, `INV-4`, or `phase-00 §delivery sequence` rather than paraphrasing what they say. Restating a rule to demonstrate you read it is the largest source of bloat and creates a second copy that will drift.

## Explicit state and ownership

- Maintain one authoritative representation of session state. Transient turn assembly may exist, but it must not become a second transcript requiring later reconciliation.
- Make lifecycle states explicit with enums/state machines. Avoid collections of loosely related booleans.
- Use stable typed identities for agents, sessions, turns, transcript items, tool calls, mail, artifacts, and surfaces.
- Keep semantic source separate from rendered presentation and provider-specific replay metadata.
- Every background task must have an owner, cancellation path, bounded resources, and observable completion. Do not detach anonymous tasks.
- Use bounded channels and queues unless an unbounded structure is justified by a proven hard upper bound.
- Cancellation, timeout, retry, partial completion, and shutdown are normal state transitions, not exceptional afterthoughts.
- Avoid mutable process-global state. If process-wide coordination is required, give it an explicit owner and testable lifecycle.

## Architectural direction

- Dependencies point inward: adapters and UI depend on semantic core contracts, never the reverse.
- The TUI consumes revisioned semantic events/snapshots and emits intents. Widgets must not call providers, storage, tools, or agent objects directly.
- Rendering is a projection of state. Do not hide business transitions inside `render`/`view` functions.
- Keep terminal event routing, focus, pointer capture, z-order, clipping, and scroll ownership centralized in the interaction/surface layer.
- Provider APIs are wire adapters over a shared semantic core; Chat Completions, Responses, and Messages do not get separate agent loops.
- Tools expose typed schemas, effects/capabilities, and bounded outputs. Tool names or prompt prose are not security boundaries.
- Agent-to-agent mail is a typed domain concept, not a fake user message or a blocking delegation result.
- Preserve exact semantic content for selection/copy. Do not reconstruct copied content from decorated or clipped terminal cells.

## Abstraction discipline

Use enough abstraction to protect a real boundary and enable genuine reuse. Do not optimize for hypothetical reuse.

Create an abstraction when at least one is true: two real callers share the same invariant and behavior; an external boundary needs a replaceable/testable adapter; a type prevents invalid states or enforces ownership/capability rules; a measured hot path needs an isolated implementation strategy.

Do not create one merely because a second implementation might exist someday, a function is long but still coherent, a design pattern has a familiar name, or a wrapper can hide an inconvenient API without improving the domain model.

Prefer a concrete implementation with a narrow seam over a general framework. It is easier to extract a correct abstraction from two working cases than to remove a speculative one embedded throughout the system.

## Anti-patterns to avoid

The following require redesign, not explanation:

- God crates, God structs, and catch-all modules that own unrelated runtime, UI, provider, and persistence concerns; generic `Manager`, `Service`, `Context`, or `Utils` buckets with unclear ownership.
- One universal event/message enum mixing provider wire events, durable events, UI animation, terminal input, and agent mail.
- Stringly typed states, roles, error categories, capabilities, IDs, or routing decisions; error strings used as machine-readable control flow.
- Multiple sources of truth with synchronization code between them.
- Hidden mutable state in globals, thread-locals, render caches, or callbacks.
- UI components that mutate runtime/session state during rendering.
- Blocking filesystem, network, model, tool, math-render, or persistence work on the TUI event loop.
- Re-layout or cloning of full transcripts for each streaming delta.
- Unbounded tool output, transcript injection, queues, retries, task spawning, or in-memory caches.
- Fire-and-forget tasks whose errors and shutdown are discarded.
- Silent fallback that changes semantics or security posture. Degradation must be typed, visible, and testable.
- Compatibility shims, deprecated parallel paths, `foo_v2`, or simultaneous replacement implementations without a bounded migration plan.
- Premature custom HTTP, database, scheduler, widget, or plugin frameworks when maintained crates satisfy the measured need.
- Feature flags that create many untested product combinations. Features isolate optional transports/integrations; they do not fork core semantics.
- Large snapshots that reviewers cannot meaningfully inspect, and tests that only confirm mocks returned what they were configured to return.

## Working discipline

Before changing code: read the routed material and the nearest module tests; check repository status and preserve unrelated user changes; identify the contract and the lowest meaningful test tier.

While changing code: keep patches scoped to one coherent outcome; add or update the test that proves the behavior, including failure and cancellation paths; run the smallest relevant test first, then the owning-crate or workspace gate proportional to risk.

Before handoff: inspect the diff for accidental dependency, generated-file, snapshot, or formatting churn; report what changed, what was tested, and what remains unverified.

Do not claim a check passed unless it was actually run in this workspace. Do not commit, publish, install globally, or mutate live user configuration unless the user explicitly requests it. Commit messages follow Conventional Commits.

## Documenting work

Keep roadmap, code, tests, and user-facing behavior aligned; stale comments and contradictory defaults are defects. Link to one source of truth instead of copying a rule into several files, and expand a document just in time rather than to look complete.

- Durable product and architecture direction → `roadmap/plexmaton.md`, which must stay short.
- Cross-cutting interaction rules → `roadmap/ui-ux.md`.
- The precise, testable definition of one mechanism → `specs/`, following `specs/README.md`. An invariant with no test is marked unproven, never left reading as fact.
- Implementation scope, evidence, and exit criteria → the active phase file. Promote a finding upward only when it changes a durable invariant or system boundary.
- A decision, plus the alternative that was seriously considered and rejected → a row in `DECISIONS.md`, which never becomes the only statement of a rule.
