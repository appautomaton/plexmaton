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
| Responses `/responses` | `tools: [{"type":"web_search"}]` | `include: ["reasoning.encrypted_content"]`, replayed as `reasoning` output items; `store: false` supported |
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
and Messages the translated one, with nothing downstream changing until a client picks a different
`api`.

## Plexmaton's Responses adapter, as it stands

The encoder already sends `store: false` and `include: ["reasoning.encrypted_content"]`, so
encrypted reasoning replay on Meta needs no change. The decoder retains any `annotations` array on
a text part as replay metadata, so `url_citation` survives a round trip already and is simply not
rendered. A `web_search_call` output item is a typed `UnsupportedEvent` on both `output_item.added`
and `output_item.done`, so a Responses turn that searched fails the step today, which is the
refusal PRV-5 records.
