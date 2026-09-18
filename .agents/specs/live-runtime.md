# Spec — Live runtime

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Live agent ownership, model-step correlation, native-tool scheduling, cancellation, reported token usage, and the executable-to-agent composition boundary |
| Depends on | [agent-loop](./agent-loop.md), [tool-admission](./tool-admission.md), [provider-adapter](./provider-adapter.md), [workspace-files](./workspace-files.md), [command-tool](./command-tool.md), [frame-loop](./frame-loop.md) |
| Proven by | `plexmaton-runtime` component tests, provider fixtures, agent correlation tests, CLI startup tests, and TUI reducer tests |

## Invariants

**LIVE-1 — One runtime owns one live agent and all its work.** The runtime accepts addressed user
input, drives `Agent::handle`, publishes its `ConversationEvent` envelopes, and performs effects; the
provider is one retained future rather than a detached task, and every native admission or
execution runs on a bounded per-call worker the runtime joins. `Drop` cancels and joins those
workers and drops the provider future; orderly shutdown remains the semantic transition. One
catalog advertises native definitions across provider dialects; skill discovery conditionally adds
the `skill` definition (SKL-2/SKL-4). Native outcomes are bounded before replay. The TUI knows only
semantic events and intents.

**LIVE-2 — Every model event names the step that requested it.** A stable typed step identity
travels on `CallModel`, streamed input and model failure. Runtime signals also carry their authorized
request-attempt identity (TIM-2); only the matching step and attempt accept output. Stale, repeated
or post-cancellation output is a typed non-delivery.

**LIVE-3 — Interrupt and shutdown cancel the same owned operation.** The agent transition first
settles its semantic debt, then the runtime signals every matching provider, admission and
execution operation and drives each to completion. One outcome wins a completion/cancellation
race; cancelled polls retain owned work; shutdown starts no new requests and joins before terminal
restoration. A cancelled shutdown call resumes retained cleanup when called again.

**LIVE-4 — Reported usage is exact, step-scoped and turn-aggregated.** Chat requests streaming usage
explicitly and Responses reads it from the terminal response. The HTTP owner retains usage with its
terminal report; TIM-2/TIM-3 commit that audit before semantic completion and derive the turn total.
Counts retain input, cached input, cache-write input, output, reasoning output and the provider's
total without recomputing subsets; cumulative addition is checked.

**LIVE-5 — Missing usage is not zero.** A provider omission, transport failure or cancellation is
an explicit coverage state (`Complete`, `Partial` or `Unavailable`) beside any reported counts.
Unknown attempts keep coverage incomplete until their terminal fact resolves it. Pre-request
context estimation is separate; monetary cost follows TIM-3's resolved pricing and known usage.

**LIVE-6 — Configuration is resolved before terminal or network ownership.** The composition root
loads provider definitions from `PLEXMATON_HOME` or the user-level default and reads the chosen key
environment variable (PRV-6). Project model selection and skill discovery follow SKL-1–SKL-4;
project permission declarations follow PER-8. It also canonicalizes the process's current directory as
the sole native workspace, resolves `rg` only from absolute `PATH` entries, and pins the current
executable plus a fixed private argument as the directory-search driver. Any failure happens before
entering the alternate screen; the selected key variable's exact name reaches the native catalog
so commands exclude it even when it has no credential-shaped suffix. Credentials, encrypted
reasoning and authorization headers have redacted diagnostics.

## Model

```text
TUI intent ─▶ CLI route ─▶ LiveRuntime ─▶ Agent::handle ─▶ ConversationEvent ─▶ TUI
                                ▲              │
                                │         CallModel(step)
                   provider future ◀───────────┤
                                │         AdmitTool / RunTool
                                │              └────▶ retained native future
                                │                           │
                                └── typed completion ◀──────┘
```

One user turn may issue several model steps. Usage belongs to a step; the turn total is their
checked sum, with coverage saying whether that sum is complete.
Cached input remains a subset of input and reasoning remains a subset of output.
CPL-6/CPL-7 own compaction attempts alongside the pending step. The runtime retains their future,
deadline and continuation across cancellation of a poll; CPL-4 acknowledgement precedes refreshing
that same step from its new checkpoint.

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
| UI closes during admission or tool execution | Shutdown cancels and joins every retained worker before terminal restoration |
| A shutdown poll is itself cancelled | The shutdown state and work remain owned; the next call resumes cleanup |
| Runtime is dropped without shutdown | Drop cancels and joins native workers and drops the provider future; it claims no semantic completion |
| A native result exceeds its final bound | A bounded typed failure enters the record; JSON is never silently truncated |
| Current directory, `rg` or internal driver cannot be pinned | Typed startup failure before terminal initialization |

## Evidence

[Named proofs](../evidence/live-runtime.md), one row an invariant.
