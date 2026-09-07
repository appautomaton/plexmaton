# Standard — Architecture

| Field | Value |
| --- | --- |
| Trigger | You are about to design, implement, or review code |
| Owns | State and task ownership, layer boundaries, abstraction criteria, and redesign triggers |

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
- Preserve exact semantic content for selection/copy. Do not reconstruct copied content from decorated or clipped terminal cells.

## Abstraction discipline

Use enough abstraction to protect a real boundary and enable genuine reuse, never for hypothetical reuse. Create one when two real callers share an invariant, an external boundary needs a replaceable adapter, a type prevents invalid states or enforces ownership, or a measured hot path needs an isolated strategy. Not because a second implementation might exist someday, a function is long but coherent, a pattern has a familiar name, or a wrapper hides an inconvenient API. Prefer a concrete implementation with a narrow seam: extracting an abstraction from two working cases is easier than removing a speculative one.

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
