# Spec — Provider adapter

| Field | Value |
| --- | --- |
| Status | Implemented; four dialects verified with hermetic tests |
| Owns | Configuration, request encoding, streaming decode, reasoning replay, and typed failure at the model-provider boundary |
| Depends on | [agent-loop](./agent-loop.md) LOOP-1, LOOP-3 and LOOP-4 |
| Proven by | `plexmaton-provider::{config,codec,sse}` tests, sanitized protocol fixtures, `plexmaton-runtime::http` tests, and `plexmaton-agent::turn` tests |

## Invariants

**PRV-1 — Wire dialects end at one semantic port.** Responses, Chat Completions, Messages and
GenerateContent encode the same ordered `ContextAtom`s and produce position-bearing semantic model events; neither owns a loop,
transcript, tool scheduler or approval path. API selection is explicit and never changes
mid-turn. A CIN-3 collaboration atom is encoded in the richest form its dialect has. No dialect in
use has a role meaning "another session said this", so all four render the resolved sources in
canonical order as one non-assistant turn whose every element names its sender and escapes its
body. A resolved atom always encodes; an unresolved reference carries no content and is refused, in
the codec and its budget path alike. Rejected: system or developer text, which gives a peer the
authority of the harness; an unattributed turn, which is indistinguishable from the user's own
words; and refusing outright until a dialect gains a native field, which withheld delegation from
every model rather than from none.

**PRV-2 — Streaming assembly is bounded and finality is explicit.** Text deltas pass through in
order; tool identity and argument fragments assemble under per-call, aggregate-step and count bounds,
and only a protocol completion event emits one whole call. Partial UTF-8, an unfinished call,
duplicate call identity, duplicate finality or a conflicting fragment is a typed malformed
response, never guessed content.
The production raw-call ceiling remains 64 KiB even when trusted canonical state reserves more;
a semantic stop is released only after the stream trailer passes validation.

**PRV-3 — Semantics and replay are exact but separate.** Every exposed reasoning artifact is
retained in output order. Bounded replay sidecars attach to text, reasoning or calls; a replay-only
block retains a part with no visible content. Responses retains item identity/status, message phase
and content grouping as well as encrypted reasoning. Messages retains complete thinking/signature and redacted
blocks; Gemini retains signatures on their original parts, signature-only text-field absence and
optional upstream call IDs. Chat
retains recognized reasoning field identity and refuses unsupported structured reasoning; its
`reasoning_details` array is admitted only when every entry is a `reasoning.text` block bearing
nothing but type, text, format and index whose concatenation equals the plain `reasoning` delta
beside it, so the admitted shape adds no content. Rejected: decoding the entries as reasoning in
their own right, which double-counts the gateways that restate the same delta twice; and admitting
signed, encrypted or summarized entries, which keeps the text and silently drops the part the codec
cannot replay.
Opaque replay carries adapter-owned route owner, codec revision and model family compatibility. It is
never rendered, copied, logged, truncated or treated as semantic text; exceeding its bound fails
the step visibly.

A reply whose compatibility is not the encoding model's is spelled from its blocks alone, never
refused: text stays text; a thought that finished is carried as text, its content being real and
readable by the next model; an interrupted one is left out, half a sentence attributed as speech
saying what the model never said, and a sidecar is what tells the two apart; a call keeps its name
and arguments, drops every shape legal only beside the replay that authenticates it — Gemini's
`thought` flag, any upstream id — and takes an id every dialect accepts, applied to the call and to
its result or to neither; a replay-only block is dropped, having been nothing but its sidecar. A
reply carrying nothing is not sent as an empty one. Sidecars stay in the record, so the model that
wrote them replays them exactly when selected again, and occupancy is measured for what ships.
Rejected: dropping foreign reasoning outright, cheaper and free of the unsigned-thought hazard, but
discarding a finished chain of reasoning the next model could have used.

Unsigned reasoning — a reply interrupted before its own replay existed — stays in the journal and
presentation, and wire encoders omit
it without fabricating replay. Only a dialect whose API requires the block refuses instead: Messages
needs a signed `thinking` block beside `tool_use`, so a tool turn that lost its signature cannot be
replayed at all. Responses identifies a reasoning item by the provider's own opaque id and accepts
an input that leaves one out, so it omits in a tool turn exactly as it does outside one. Rejected:
one rule for both, which made a single interrupted tool turn poison every later request in its
conversation — a session the user could read and never continue.

**PRV-4 — The semantic record is replay authority.** Every request is rebuilt from the record,
including exact provider replay sidecars and tool-call identities. The same selected journal path,
resolved configuration (including AGI-4's current workspace instruction snapshot), ordered tool
schemas and output cap produce the same wire request. Gemini local call IDs are scoped
to the request attempt; only retained upstream IDs enter the wire call/result pair. A response-level
ID is diagnostic metadata only: the selected proxy reports `store: false`, and an unavailable `previous_response_id`
must not strand the session or create an adapter-private transcript.
Checkpoint summaries encode as harness-supplied user context; CPL-3/CPL-5 own their projection,
and CPL-6 keeps the summarizer's reasoning/replay in its audit rather than the replacement.

**PRV-5 — Stops and failures stay typed.** Each protocol maps its declared completion, output-limit,
refusal, rate-limit, context-limit, provider failure, transport and malformed states without matching presentation text.
An HTTP rejection or declared stream error without a more specific category is `ProviderFailed`;
`Transport` means the response could not arrive intact, and `Malformed` means decoding failed.
Unknown additive wire events are observable and ignored only when they carry no semantic content;
an unknown content-bearing event fails rather than silently losing output. A populated field earns
that exemption only by being read and named at the surface it appears on, never by resembling one
that was: Chat admits `provider_metadata`, the gateway accounting — cost, cache counts, routing
attempts — that rides the final delta beside `finish_reason`, and admits it nowhere else.
Rejected: refusing it as unexamined, which was right in principle and in practice discarded a whole
completed answer at its last chunk, on every gateway that sends it.
A tool the provider ran on its own side falls the other way: a `server_tool_use` block and a
non-zero server-tool count are content this harness never admitted, never bounded and cannot show,
and a turn that absorbs them quietly reports work its own record does not contain. Rejected:
admitting them as opaque replay, which round-trips correctly and leaves the reader an answer whose
sources appear nowhere in the conversation.

**PRV-6 — Configuration names data, never authority.** `~/.plexmaton/config.toml` separates named
provider routes from their named models and selects one exact provider/model pair. A route owns its
base URL, credential environment and default API; a model owns its wire/display identity, optional
API override, optional reasoning controls and a declared allowed-effort subset, stable instructions, cache intent, context/output/reserve
limits, compaction retention target, estimator and optional price
snapshot. Resolution yields one immutable credential-blind value: omitted estimators become an
explicit versioned default, while omitted pricing remains unavailable. Selection never uses fuzzy
names or URL inference. `PLEXMATON_HOME` redirects the whole root for isolated development; keys
never enter the file, diagnostics, repository or a native command's environment, and invalid input
fails before network work begins. SKL-1 permits a narrow project model-selection layer without
project provider definitions or credential changes. An allowed-effort declaration must be nonempty,
unique and encodable by the dialect; an explicit configured effort must belong to it. Rejected: a combined provider/model profile,
inline keys, and untyped merging of project configuration into provider authority.

[AGI-3/AGI-4](./agent-instructions.md) add a bounded workspace snapshot to that immutable request
environment as user context, separately from configured system instructions and tool authority.

**PRV-7 — Local bounds do not trust upstream hints.** Provider token limits may be forwarded but
are not memory or context boundaries. The stream owner bounds retained output and replay locally,
has one cancellation path, and reports truncation/cancellation explicitly; the selected proxy's
handling of a token hint is not a security invariant.

## Model

```text
ModelRequest (session identity + atoms) + resolved model + tool schemas
                  │
        explicit protocol codec
    ┌─────────────┼─────────────┬────────────────┐
    │             │             │                │
 Responses       Chat        Messages     GenerateContent
    └─────────────┴─────────────┴────────────────┘
                  ▼
      ModelEvent + ProviderReplay
```

The development route is an OpenAI-compatible proxy on loopback. Its port is the developer's own,
so it belongs in their `providers.toml` rather than here. That route's `luna` model uses
`gpt-5.6-luna` over Responses, while Chat Completions remains an explicit model API choice.
Rejected: xAI as the first provider, because it is not the endpoint Plexmaton's local development
loop exercises. Rejected: automatic fallback, because replay and failure semantics change across
APIs.

## Failure modes

| Situation | Response |
| --- | --- |
| Missing config, provider/model selection or key environment variable | Typed configuration error before transport construction |
| Tool arguments exceed their bound or the stream ends mid-call | Malformed step; no partial call reaches admission |
| Responses continuation cannot resolve a response ID | Rebuild stateless input from the authoritative semantic record and exact replay items |
| Encrypted reasoning exceeds its bound | Fail the step; never truncate an opaque replay capsule |
| Stream is cancelled or disconnects | One terminal typed outcome; no detached reader remains |
| Effort is outside the selected dialect's grammar | Typed configuration error before transport construction; no silent downgrade |
| Provider rejects a model-specific option or reports a server error | `ProviderFailed`, retained in request accounting; JRN-8 owns retry eligibility |

## Supported model scope

Provider work targets the current frontier series requested for this stage: GPT-5.6
(Sol/Terra/Luna), GPT-6 Astra, Claude Opus 5/Fable 5/Sonnet 5, and Gemini 3.8 Flash. Older models
are not a reason to retain compatibility branches. Configuration still names exact route/model
IDs, including proxy aliases; it does not guess backend capabilities from those names.

| Target | Request constraints | Official reference |
| --- | --- | --- |
| GPT-5.6 Sol/Terra/Luna | Explicit reasoning effort; no `minimal` | [Sol](https://developers.openai.com/api/docs/models/gpt-5.6-sol), [Terra](https://developers.openai.com/api/docs/models/gpt-5.6-terra), [Luna](https://developers.openai.com/api/docs/models/gpt-5.6-luna) |
| GPT-6 Astra | Use Responses for tools; `none` is not supported by the native model | [Model guide](https://developers.openai.com/api/docs/guides/latest-model) |
| Claude Opus 5/Fable 5/Sonnet 5 | Adaptive thinking and effort; no manual budget. Fable does not support disabled thinking | [Thinking](https://platform.claude.com/docs/en/build-with-claude/thinking) |
| Gemini 3.8 Flash | Native GenerateContent supports `low`/`medium`/`high`; no `minimal`, thinking budget or candidate-count setting | [3.8 guide](https://ai.google.dev/gemini-api/docs/generate-content/latest-model) |

The Chat adapter serves an explicitly selected compatible route. CLIProxyAPI's inspected
[server routes](https://github.com/router-for-me/CLIProxyAPI/blob/main/internal/api/server_routes.go)
expose the Gemini facade at `/v1beta/models/*action`, and its
[access provider](https://github.com/router-for-me/CLIProxyAPI/blob/main/docs/sdk-access.md)
accepts `X-Goog-Api-Key`. Select `google_generate_content` with a `/v1beta` proxy root; use the model
ID advertised by that instance. The proxy's
[Antigravity executor](https://github.com/router-for-me/CLIProxyAPI/blob/main/internal/runtime/executor/antigravity_executor.go)
owns OAuth and the CloudCode `/v1internal` upstream path. No Vertex/project wrapper is added to
Plexmaton's native request. This is source evidence, not verification of the installed proxy version
or its model access. Google's managed Antigravity agent uses Interactions and is a separate surface.

## Request configuration

`base_url` includes the API version path; selection never guesses a dialect from the URL.
[Provider examples](../../examples/providers.toml) show the route/model configuration.

| `api` | Path appended to the configured root | Authentication |
| --- | --- | --- |
| `openai_responses` | `responses` | Bearer key |
| `openai_chat_completions` | `chat/completions` | Bearer key |
| `anthropic_messages` | `messages` | `x-api-key`, `anthropic-version: 2023-06-01` |
| `google_generate_content` | `models/<id>:streamGenerateContent?alt=sse` | `x-goog-api-key` |

Model `instructions` defaults to empty and is bounded at 64 KiB. It encodes as Responses
`instructions`, a Chat system message, Messages `system`, or Gemini `systemInstruction`, and enters
the request-environment fingerprint. It comes from explicit user configuration, not project
instruction discovery or a second persisted transcript.

`reasoning_effort` defaults to `default`, which leaves the effort level to the provider. OpenAI
receives explicit effort directly. Messages uses disabled thinking for `none`; otherwise it requests
`thinking: {type: adaptive, display: summarized}`, including at default effort, and sets
`output_config.effort` only for an explicit level. This requests the
[visible summary](https://platform.claude.com/docs/en/build-with-claude/thinking#controlling-thinking-display)
without promising that every request produces one. Gemini always requests
[thought summaries](https://ai.google.dev/gemini-api/docs/generate-content/thinking#thought-summaries)
with `includeThoughts: true`. It accepts only `default`/`low`/`medium`/`high`, omitting `thinkingLevel`
for default effort; `none` is not silently mapped to low. The legacy `thinking_budget_tokens` field and `minimal` effort are rejected
before transport construction. `max_output_tokens` remains the output cap. Rejected: legacy manual
thinking paths, because they are outside the frontier target set. Per-model restrictions within a
dialect remain the endpoint's contract; unsupported options never trigger a silent downgrade.

`allowed_reasoning_efforts` optionally declares a model's available explicit levels, in configuration
order, from `none`, `low`, `medium`, `high`, `xhigh`, `max`. The list cannot be empty or contain
duplicates, `default`, unknown levels, or levels the selected dialect cannot encode. A configured
explicit `reasoning_effort` must belong to it. Omitting the list leaves capabilities unknown; the
harness does not infer a model's support from its name. `default` remains a separate omission of
the provider effort field and is valid with any declared subset. This metadata is not sent on the
wire and does not itself enable interactive effort changes.

`prompt_cache` defaults to `automatic`: OpenAI gets a 64-character hash of canonical session
identity as `prompt_cache_key`; Messages gets top-level `cache_control: {type: ephemeral}`; Gemini uses
implicit caching. `disabled` suppresses harness-added hints, not provider-internal caching. The key
is stable across turns, attempts and resume. Cache affinity follows each API's grammar and still
depends on the provider and exact prefix.

`compaction_keep_recent_tokens` defaults to `20000` and must be positive. CPL-3 owns its effective
retained-tail budget. This planning metadata changes no wire field or request-environment
fingerprint. CPL-5 keeps existing checkpoint context fixed; a later compaction may select a new cut.

Messages [automatic caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching#automatic-caching)
targets the last cacheable block, including tools and history, without requiring `system`.
Rejected: block-only caching as the default, because the native API already advances its cache
point through the conversation; legacy Bedrock and gateway compatibility are outside this grammar.

## Native content and usage

| Dialect | Call/result grammar | Inclusive input | Generated output |
| --- | --- | --- | --- |
| Responses | `function_call` / `function_call_output`, joined by `call_id` | `input_tokens` | `output_tokens`, including reasoning |
| Chat | assistant `tool_calls` / role `tool`, joined by `tool_call_id` | `prompt_tokens` | `completion_tokens`, including reasoning |
| Messages | assistant `tool_use` / user `tool_result`, joined by `tool_use_id` | `input_tokens + cache_read_input_tokens + cache_creation_input_tokens` | `output_tokens`; optional `thinking_tokens` breakdown |
| GenerateContent | model `functionCall` / user `functionResponse`, optional upstream `id` | `promptTokenCount` | `candidatesTokenCount + thoughtsTokenCount` |

OpenAI totals are retained and checked. Messages has no total field, so its adapter derives the
sum once. Gemini validates a provided total or derives it from its components. Messages omitted
cache counters and Gemini omitted protobuf counters denote zero; OpenAI missing optional details
remain unknown. Gemini has no separately accounted cache-write category, so that category is zero.
Cumulative stream counters replace prior observations. A terminal persists the
latest observed report; cancellation cannot invent a later bill. BUD-2 owns measurement eligibility;
TIM-3 owns durable cost and aggregate coverage.

Messages `output_tokens_details.thinking_tokens` is an optional final-`message_delta` field in the
[thinking pricing reference](https://platform.claude.com/docs/en/build-with-claude/thinking-steering-and-cost#pricing).
Its absence leaves `reasoning_output` unknown and usage partial; it does not prevent pricing a
completed request. Nonzero hosted-tool usage is refused by both native adapters.

The native surface covers text, exposed thinking and local function tools. Multiple candidates,
unsupported content-bearing blocks and incomplete calls/signatures fail visibly. A cancelled or
failed unsigned thinking summary stays visible and durable but cannot become a signed wire block.
Messages omits an assistant message if filtering leaves no content; Responses omits the unsigned
reasoning item. Signed blocks remain unchanged, and unsigned reasoning in a tool batch is refused.
This follows Messages' [prior-turn thinking rules](https://platform.claude.com/docs/en/build-with-claude/thinking#preserving-thinking-blocks). Messages context
exhaustion maps to `ContextLimit` while retaining final usage; request rejection remains a model
error. `pause_turn` is explicitly unsupported because it requires a server-tool continuation;
`stop_reason` determines completion/refusal. Hosted tools, media, OAuth/subscription login, Vertex, Bedrock and Interactions are outside
these grammars.

Gemini's declared invalid-call, unexpected-call, tool-count, missing-signature and malformed-response
finishes become `ProviderFailed` with their wire code, retaining observed usage and dispatching no
calls. `TOO_MANY_TOOL_CALLS` is a tool-execution failure, not an output-token limit. Escalation and
account-policy stops map to `Refused`; image reasons remain unsupported. The
[v1beta discovery schema](https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta)
owns the exact enum. Stream error classification lives in the adapter. An in-stream rate limit
normally has no retry delay; it retains `Retry-After` only if the initial HTTP response supplied
that header. The synthetic 200-with-header fixture proves propagation, not provider behavior.

Generic native HTTP validation errors remain `ProviderFailed`; context length is inferred only from
a declared stable code, never provider message prose. Chat delta and Gemini part additions with
null, empty-array or empty-object values are ignored with field-name-only debug diagnostics.
Populated unimplemented fields are refused, including Chat annotations/legacy calls and Gemini
media metadata; their values never enter diagnostics. A non-tool finish with calls
refuses the batch, and inconsistent usage fails decoding under PRV-2/PRV-5. Rejected: treating these
as successful tool output or trustworthy accounting, because neither can be reconstructed safely.

Wire references: [Messages](https://platform.claude.com/docs/en/api/messages/create),
[Messages streaming](https://platform.claude.com/docs/en/build-with-claude/streaming),
[Messages stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons),
[GenerateContent](https://ai.google.dev/api/generate-content), and
[Gemini signatures](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures).

## Evidence

[Named proofs](../evidence/provider-adapter.md), one row an invariant.
