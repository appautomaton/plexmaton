# Provider dialects spike

| Field | Value |
| --- | --- |
| Status | Implementation verified with hermetic tests; live compatibility unverified |
| Read when | Changing provider replay, native wire grammars, cache affinity or token normalization |
| Question | What evidence supports faithful text, reasoning and local-tool conversations across our target APIs? |
| Decision | Support Responses, Chat Completions, Messages and native Gemini GenerateContent; retain one semantic journal and loop |
| Contract | [provider-adapter](../../specs/provider-adapter.md) PRV-1 through PRV-7; [context-budget](../../specs/context-budget.md) BUD-2/BUD-3; TIM-3/TIM-4 |

## Corpus

Inspected local source on 2026-09-05. Plexmaton work starts at
`01ecf0bc38c38f5d463484db87c40ecdf094bbc8` in branch `spike/provider-adapter-parity`,
under `.worktrees/provider-adapter-parity/`. Reference code was inspected; its test suites were not run.

| Local repository or snapshot | Revision | Relevant source |
| --- | --- | --- |
| pi-mono (`pi-arcweld/pi-mono`; origin `earendil-works/pi`) | `853a80d26c90a14c1886f0ebb8ffaae133ca2185` | `packages/ai/src/api/{openai-responses-shared,openai-completions,anthropic-messages,google-generative-ai,google-shared}.ts` |
| DeepSeek Harness (`dsh`) | `47f943859bef60e4160492346772ded9b24f765a` | `packages/llm/llm-deepseek/src/{serialize,translate}.ts`; `llm-pi-ai` reuses pi |
| Kimi Code (`kimi-code`) | `17dfd49768f753a4f0fe97d8e7d3317dab560575` | `packages/kosong/src/providers/{anthropic,google-genai,openai-responses,openai-legacy}.ts` |
| Codex | `316795b3cf2a45e90d121d9f46499d4658b2645c` | `codex-rs/protocol/src/models.rs`; `codex-rs/core/src/client.rs` |
| Grok Build | `72a61251fcffb464bcc687aeb5a998e5a98ec0c9` | `xai-grok-sampling-types/src/conversation/{responses,chat_completions}.rs`; `xai-grok-sampler/src/stream/chat_completions.rs` under `crates/codegen/` |
| Claude Code | Mapped `cc/2.1.88` snapshot | `src/services/api/claude.ts`, `src/utils/tokens.ts`; source-map extraction, not a public full-source Git checkout |

## What the comparison established

**Replay must cover more than reasoning.** pi records Responses message ID and phase through
`textSignature`; Codex has explicit optional message ID/phase fields. Google puts signatures on
text and function-call parts too. Plexmaton's original reasoning-only anchor rejected that shape,
and its Responses decoder discarded completed message metadata. The implementation now retains
metadata at the actual block, including returned item status and function item ID, and uses an
invisible `ReplayOnly` block when no semantic text exists.
The [OpenAI phase guidance](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-6-astra#phase-parameter)
provides the protocol reason to preserve phase during manual history replay.

**Provider support is the whole boundary.** pi and Kimi implement request construction, streaming,
reasoning replay, usage and provider configuration together. Grok's Responses conversion preserves
mixed output order; its Chat conversion folds reasoning into an assistant message. DeepSeek's
native adapter retains `reasoning_content` for tool-call continuations and defers terminal usage to
`[DONE]`. These are evidence for wire-specific handling behind PRV-1, not separate agent loops.
Plexmaton now preserves the recognized Chat reasoning field name and explicitly rejects unsupported
structured reasoning rather than silently discarding it.

**Usage shapes are different.** pi and DeepSeek Harness expose disjoint uncached/cache categories;
Plexmaton's input is inclusive. Claude Code updates cumulative message usage while its current
context reads the latest assistant request; session accounting is a separate fold. pi's Google
adapter includes thoughts in generated output, while the inspected Kimi Google usage mapper counts
only response candidates. Reference implementations are comparison evidence, not interchangeable
accounting specifications. The durable formulas now live in [provider-adapter](../../specs/provider-adapter.md).

## Native protocol evidence

Messages has a top-level system slot, assistant `tool_use`, user `tool_result`, signed thinking and
redacted thinking. Its stream has explicit message/block start and stop events; usage in
`message_delta` is cumulative. We follow the native grammar and retain only complete signed blocks.
Sources: [Messages](https://platform.claude.com/docs/en/api/messages/create),
[streaming](https://platform.claude.com/docs/en/build-with-claude/streaming), and
[thinking](https://platform.claude.com/docs/en/build-with-claude/thinking).

Gemini has enough evidence for a bounded native implementation: Google REST documentation plus
independent pi and Kimi request/replay paths. The selected API is specifically
`models/<id>:streamGenerateContent?alt=sse` under the configured `v1beta` root; a version string
alone does not identify a protocol. Google's documentation also describes a separate Interactions
API, which this adapter does not implement. GenerateContent's function call is complete in a part;
its optional upstream ID must be returned with the corresponding function response. We derive local
call identity from the request attempt and retain upstream identity separately, so repeated short
IDs across requests cannot collide locally or alter replayed wire IDs.
Sources: [GenerateContent reference](https://ai.google.dev/api/generate-content),
[thought signatures](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures),
[authentication](https://ai.google.dev/api), and
[Interactions comparison](https://ai.google.dev/gemini-api/docs/migrate-to-interactions).

## Cache and measurement decisions

OpenAI cache routing uses a bounded hash of canonical session identity, stable across steps, turns
and resume. Messages uses native top-level automatic cache control, even without system content; Gemini uses implicit caching.
Disabling harness hints does not disable a provider's implicit cache. Stable ordering and exact
replay remain necessary; none of these settings proves retained physical KV state.
Sources: [OpenAI caching](https://developers.openai.com/api/docs/guides/prompt-caching) and
[Messages caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching).

A request's input measures its entire submitted context. Latest cache/input and summed cache/input
answer different questions. BUD-2 accepts an individual measured input with missing optional
details. Cost requires protocol completion and its priced categories; the reasoning split is
optional. Cancelled or failed requests retain observed usage with unavailable cost, even when all
fields are present. Aggregate partial usage is never a context anchor.

## Evidence and limits

On base `3983bca`, 795 parallel tests and static/corpus gates pass, including the JRN-4
inherited-descriptor lock regression.

Reviewed real-renderer frames: provider failure at [wide](./frames/provider-failure-wide.txt),
[medium](./frames/provider-failure-medium.txt), [narrow](./frames/provider-failure-narrow.txt);
interrupted thinking at [wide](./frames/interrupted-thinking-wide.txt),
[medium](./frames/interrupted-thinking-medium.txt), [narrow](./frames/interrupted-thinking-narrow.txt).
Hermetic evidence includes decoded tool round trips, Responses phase and part grouping, native
signatures through real JSONL write/reopen, session cache-key stability, scoped Gemini IDs,
unsupported-content failures, native HTTP headers, typed provider errors, rate limits and cancellation. The
[provider spec](../../specs/provider-adapter.md) owns the test index. Run from the worktree:

```sh
cargo test -p plexmaton-provider
cargo test -p plexmaton-runtime --test provider_replay
cargo test --workspace
```

HTTP fixtures use loopback servers, never real model services.
Live model compatibility, realized cache hits and provider billing remain unverified. This scope
covers API-key text/thinking/local-function tools; it does not claim hosted tools, media, OAuth,
Vertex/Bedrock or arbitrary vendor-option parity. Older journals cannot recover metadata that their
writer never recorded.
