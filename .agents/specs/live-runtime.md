# Spec — Live runtime

| Field | Value |
| --- | --- |
| Status | Designed for Phase 01 stage 2 slice 6; unproven |
| Owns | Live agent task ownership, model-step correlation, cancellation, reported token usage, and the executable-to-agent composition boundary |
| Depends on | [agent-loop](./agent-loop.md) LOOP-1, LOOP-4 and LOOP-6; [provider-adapter](./provider-adapter.md) PRV-1, PRV-5 through PRV-7; [frame-loop](./frame-loop.md) FR-1 |
| Proven by | Unproven; Phase 01 stage 2 slice 6 |

## Invariants

**LIVE-1 — One runtime owns one live agent and all its work.** The runtime accepts addressed user
input, drives `Agent::handle`, publishes its `SessionEvent` envelopes, and performs effects; every
provider task and bounded channel has that runtime as owner, an observable terminal result, and a
join path. The TUI knows only semantic events and intents, while the provider knows no runtime or
projection.

**LIVE-2 — Every model event names the step that requested it.** A stable typed step identity
travels on `CallModel`, streamed input and model failure. The agent accepts output only for its
currently open matching step; stale, repeated or post-cancellation output is a typed non-delivery
and can never attach to a later turn.

**LIVE-3 — Interrupt and shutdown cancel the same owned operation.** The agent transition first
settles its semantic debt, then the runtime signals the matching provider task and awaits its end.
Exactly one terminal outcome wins a completion/cancellation race, no new request starts during
shutdown, and dropping the TUI never detaches network work.

**LIVE-4 — Reported usage is exact, step-scoped and turn-aggregated.** A completed provider stream
emits exactly one `ReportedUsage` before its stop; Chat requests streaming usage explicitly and
Responses reads it from the terminal response. Counts retain input, cached input, cache-write input,
output, reasoning output and the provider's total without recomputing subsets; checked addition
produces the turn total across tool-loop steps.

**LIVE-5 — Missing usage is not zero.** A provider omission, transport failure or cancellation is
an explicit coverage state (`Complete`, `Partial` or `Unavailable`) beside any reported counts.
Reported usage measures completed consumption only; pre-request context estimation and monetary
pricing are separate mechanisms and never inferred from it.

**LIVE-6 — Configuration is resolved before terminal or network ownership.** The composition root
uses `PLEXMATON_HOME` or the user-level default and reads the chosen key environment variable; it
never searches a project `.plexmaton/`. Invalid configuration fails before entering the alternate
screen, and credentials, encrypted reasoning and authorization headers have redacted diagnostics.

## Model

```text
TUI intent ─▶ CLI route ─▶ LiveRuntime ─▶ Agent::handle
                                │              │
                         owned task ◀── CallModel(step)
                                │
                  Streamed/Failed(step) + ReportedUsage
                                │
                                └─▶ Agent ─▶ SessionEvent ─▶ TUI
```

One user turn may issue several model steps once tools exist. Provider usage belongs to a step;
the turn total is the checked sum of reports, with coverage saying whether that sum is complete.
Cached input remains a subset of input and reasoning remains a subset of output.

## Failure modes

| Situation | Response |
| --- | --- |
| Config or key is absent | Typed startup failure before terminal initialization |
| HTTP request or SSE decode fails | Matching step receives one typed failure and the task terminates |
| `Ctrl-C` races with the final SSE event | One terminal transition wins; the other is stale by step identity |
| A cancelled stream returns a late delta | Typed non-delivery; no record, frame or later turn changes |
| Provider omits usage | Answer remains valid and usage coverage is unavailable, never zero |
| One usage field or turn sum overflows | Malformed report; no wrapped or saturated count is displayed |
| UI closes during a request | Shutdown cancels and joins the request before terminal restoration completes |

## Evidence

| Invariant | Proven by |
| --- | --- |
| LIVE-1 | Unproven; slice 6 deterministic runtime component test and crate-graph gate |
| LIVE-2 | Unproven; slice 6 late-event and new-turn correlation tests |
| LIVE-3 | Unproven; slice 6 cancellation-race and shutdown tests |
| LIVE-4 | Unproven; slice 6 Chat and Responses usage fixtures plus multi-step turn test |
| LIVE-5 | Unproven; slice 6 omitted-usage, failure and cancellation tests |
| LIVE-6 | Unproven; slice 6 configuration and pre-terminal startup tests |
