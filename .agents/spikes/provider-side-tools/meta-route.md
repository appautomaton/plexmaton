# Meta route evidence

Two kinds of fact here, kept apart. **Observed** rows were measured on this host. **Documented**
rows come from Meta's own protocol pages, read 2026-09-21, and have not yet been observed here: a
direct probe of `api.meta.ai` was refused by the session's permission classifier as credential
extraction, and the gateway has no native Meta route configured to probe through.

## Meta's three surfaces, from its documentation

Base URL `https://api.meta.ai/v1`. Both `muse-spark-1.3` and `muse-spark-1.3-contributor` are
served on all three.

| Surface | Hosted search | Reasoning across turns |
| --- | --- | --- |
| Chat Completions `/chat/completions` | **none**: "Search grounding is not available on Chat Completions" | **none**: "Chat Completions does not carry reasoning across turns"; the response carries `message.content` only |
| Responses `/responses` | `tools: [{"type":"web_search"}]`, **observed** through the gateway's existing route | `include: ["reasoning.encrypted_content"]`, replayed as `reasoning` output items, `store: false`; encrypted content **observed** arriving through the gateway |
| Messages `/messages` | observed working through the gateway with `web_search_20250305` | `redacted_thinking` blocks, observed |

So the Chat Completions facade's two holes measured in [gateway evidence](./gateway-evidence.md)
are not the gateway's translator alone: Meta's own Chat Completions has neither. The translation
layer removes what the protocol would carry, and the protocol carries neither of the two things
this stage needs.

## What hosted search returns on Responses, documented

A `web_search_call` output item with `id`, `type`, `status`, and the answer in a `message` whose
`output_text` carries `annotations` of type `url_citation` with `url`, `title`, `start_index`,
`end_index`. Raw results are off by default and requested with
`include: ["web_search_call.results"]`, after which the item carries `results[]` of
`{type: "text_result", title, url, snippet}`. `search_context_size` takes `low`, `medium`, `high`.
Adding the tool does not force a search. On replay a `web_search_call` may be sent with `id` null.

This reverses the earlier reading that hosted search returns queries and never findings. That was
true of the Messages surface as observed here, and of OpenAI's inline path as Codex models it. It
is not true of Meta's Responses surface, which is the one route on this host that would return
sources a transcript could show.

## Responses through the existing route, observed

Measured 2026-09-21 through `127.0.0.1:5233/v1/responses` with the gateway still routing Muse via
`claude-api-key`, so every request below was translated to Messages and back. `store: false`,
`include: ["reasoning.encrypted_content"]`, `reasoning.effort: high`, `max_output_tokens: 2048`.

| Request | Output items | Answer |
| --- | --- | --- |
| `tools: [{"type":"web_search"}]` | 5 `reasoning` with `encrypted_content`, 5 `web_search_call`, 6 `message` | **1.98.1** |
| same, plus `include: web_search_call.results` | 2 `reasoning`, 1 `web_search_call`, 2 `message` | **1.98.1** |
| no tools, 2048 budget | none; `status: incomplete`, `incomplete_details.reason: max_output_tokens` | none |
| no tools, 8192 budget | `reasoning`, `message` | 1.98.0 from memory |
| `Say: ok`, 2048 budget | `reasoning`, `message`; 16 output tokens | ok |

So a Plexmaton Session on `openai_responses` reaches hosted search on Muse today, with no gateway
change: the tool declaration crosses the translator, `web_search_call` items come back with
`action: {"type":"search","query":…}`, and encrypted reasoning is present for replay. The empty
control run is a budget exhausted while thinking, reported as the typed incomplete state PRV-5
already maps, not a route defect.

What the translated route loses, each observed: `annotations` is `[]` on every text part and no
`web_search_call` carries `results`, with or without the include; two of five calls arrived as
`search` with an empty `query`, where the Messages surface had shown `open_page` actions, so the
translator flattens action types; and `output_tokens_details.reasoning_tokens` is `0` while the
Messages surface reports `thinking_tokens`. A decoder must accept an empty query rather than fail
on it.

The live dev config now runs muse on `openai_responses`, switched 2026-09-21 and verified through
Plexmaton itself rather than curl: main's binary at `c7bdf0a`, an ephemeral conversation, muse
chosen through `/model`, two turns. "17 times 23" answered 391 in 2.2 s; "add 9 to your previous
answer" answered 400 in 1.0 s, so the first turn's output replayed through the gateway's
translation and back intact; usage reported 2.1k in, 123 out; nothing on screen read as an error.
Whether the encrypted reasoning items survived that replay is not observable from the screen and
is untested.

## A replayed search item must not carry its id, observed

Measured 2026-09-21 through the same route, replaying one `web_search_call` in `input` ahead of the
assistant's answer, then asking a follow-up.

| Replayed item | Result |
| --- | --- |
| none | 200 |
| `id` + `status` + `action` with `query` and `queries` | 400, "content block `web_search_tool_result` is not supported on `assistant` messages" |
| same without `id` | **200**, answered |
| `id` present, any action shape or none | 400, same message |

The gateway's Responses-to-Claude translator turns any replayed item id into a fabricated result
block on the assistant message, which the upstream refuses. Meta documents the id as optional on
replay. So the encoder replays the item with its status and action and keeps the id in the sidecar
only. Found by driving this branch's binary through a search turn and a follow-up: the first
answered 1.98.1, the second returned "provider returned HTTP 400" until the id was dropped.

## The gateway already has a native Meta route

`cli-proxy-api` declares `meta-api-key` as a provider kind. `MetaKey` is a type alias of `CodexKey`,
so it takes `api-key`, optional `base-url` (default `https://api.meta.ai/v1`), `models[]` with
`name`/`alias`, and `excluded-models`. Its executor speaks Responses to the upstream, translating
inbound Chat Completions, Messages or Responses into it and back. On the way out it deletes only
`generate`, `prompt_cache_retention`, `safety_identifier`, `stream_options` and `client_metadata`,
and strips reasoning items whose ids the upstream would not know when `store` is false. `tools`,
`include` and `store` pass through untouched.

The stack routes Muse through `claude-api-key` instead, so today every surface except Messages is a
translation. A `meta-api-key` fragment with the same alias would make Responses the native surface
and Messages the translated one, with nothing downstream changing. That switch is the owner's to
make and is not required by this stage: it upgrades citations, results, exact action types and
reasoning counts, and changes nothing about which items a decoder has to accept.

## Plexmaton's Responses adapter, as it stands

The encoder already sends `store: false` and `include: ["reasoning.encrypted_content"]`, so
encrypted reasoning replay on Meta needs no change. The decoder retains any `annotations` array on
a text part as replay metadata, so `url_citation` survives a round trip already and is simply not
rendered. A `web_search_call` output item is a typed `UnsupportedEvent` on both `output_item.added`
and `output_item.done`, so a Responses turn that searched fails the step today, which is the
refusal PRV-5 records.
