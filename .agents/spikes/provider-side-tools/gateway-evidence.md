# Gateway evidence

Measured 2026-09-20 against the local gateway, not read from documentation. Every row below was
observed; nothing here is inferred from a vendor document.

Route under test: `cli-proxy-api` on `127.0.0.1:5233`, built and configured from
`~/dev/ai/cpa-local-stack`. Reproduce with the key in `PLEXMATON_LOCAL_API_KEY`:

```sh
curl -s -X POST http://127.0.0.1:5233/v1/messages \
  -H "content-type: application/json" \
  -H "x-api-key: $PLEXMATON_LOCAL_API_KEY" -H "anthropic-version: 2023-06-01" \
  -d '{"model":"muse-spark-1.3","max_tokens":1024,
       "tools":[{"type":"web_search_20250305","name":"web_search"}],
       "messages":[{"role":"user","content":"Search: latest stable Rust release."}]}'
```

## The model is not what its route says

`muse-spark-1.3` resolves to `muse-spark-1.3-contributor` at `https://api.meta.ai`, exposed through
the gateway's Claude-compatible route. It is a **Meta** model speaking the Messages protocol. The
`owned_by: anthropic` in `/v1/models` names the protocol, not the vendor, and the `msg_` ids,
`redacted_thinking` blocks and Anthropic SSE sequence are all wire format.

A test resting on this route proves an adapter speaks Messages correctly. It proves nothing about
Anthropic's own service, and must say so rather than borrowing the vendor's name.

## Reasoning effort

The endpoint declares `[minimal, low, medium, high, xhigh, max]` in its own error text. The route
accepts four of them.

| Level | `/v1/chat/completions` | `/v1/messages` |
| --- | --- | --- |
| `none`, `minimal` | 400 | 400 |
| `low`, `medium`, `high`, `xhigh` | 200 | 200 |
| `max` | 400 | 400 |

Sending `reasoning_effort: "minimal"` to the OpenAI facade returns an error naming
`` unsupported `output_config.effort` value ``, so that facade rewrites the OpenAI field into the
Messages one. Both routes land on a single parameter, and changing wire format does not unlock a
level.

## Hosted search

Works, on the Messages route only.

| Tool type | Result |
| --- | --- |
| `web_search_20250305` | accepted; `max_uses` is rejected as a field on this route |
| `web_search_20251022` | `tool type not supported` |
| `web_fetch_20250910` | `tool type not supported` |

One tool covers both actions: `{"type":"search","query":…}` and `{"type":"open_page","url":…}`.
A single observed turn issued three searches and one page open, narrating between them.

pi declares `max_uses: 8` for its `anthropic` and `cli-proxy-api-anthropic` routes, so that field's
availability is route-specific rather than absent from the tool.

## Both dialects offer one search tool, and no fetch tool

Responses on this gateway, model `gpt-5.6-luna`:

| Tool type | Result |
| --- | --- |
| `web_search` | 200; output items `reasoning`, `web_search_call`, `message` |
| `web_search_preview` | 200; same shape, legacy alias |
| `web_fetch`, `fetch` | 400 `Unsupported tool type` |
| `code_interpreter`, `file_search` | 400 `Unsupported tool type` |

**Fetching a page is an action inside the search tool, not a tool of its own,** and both dialects
agree on that. Responses returns `action: {"type":"search","queries":[…]}`; Codex models exactly
`Search`, `OpenPage`, `FindInPage` and a serde catch-all, so a new action needs no release. Messages
on `muse-spark-1.3` issued three `search` actions and one `open_page` in a single observed turn.

So a harness modelling hosted search has one shape to carry, not one per vendor. Only the wrapper
differs: Responses reports the queries as a `web_search_call` output item, Messages as
`server_tool_use` blocks. Neither reports the findings.

Those 400s are this gateway's vocabulary and do not establish what a vendor's own API offers.
Codex, which talks to OpenAI directly, likewise models no fetch tool — two independent signals
rather than one.

Hosted search is the only server-side tool this gateway accepts on either dialect, which bounds
what a first implementation has to handle.

## The results never arrive

**The gateway forwards the queries and never the findings.** Verified on both transports.

A streaming census of one complete search turn: 4 `text` blocks, 3 `server_tool_use` blocks,
**zero** `web_search_tool_result` blocks, and no `server_tool_use` counter in `usage`.

So a transcript built from this route can show what was searched and what the model concluded, and
never the sources. Codex reaches the same place from a different vendor: its inline
`WebSearchCall` path sets `results: None` because the Responses call returns the query alone. The
hole looks like the shape of inline hosted search rather than a local defect, which is the reason
the spike should not design around recovering results.

Unexamined: whether the gateway's strict cloaking strips them.
`upstream/cli-proxy-api/internal/runtime/executor/claude_executor_cloaking.go` handles
`server_tool_use` and is where to look.

## Replay has no structural blocker

Both arms return 200 and answer a follow-up correctly.

| Assistant turn sent back | Result |
| --- | --- |
| verbatim, including `server_tool_use` blocks | 200, follow-up correct |
| with `server_tool_use` stripped | 200 |

This route accepts a `server_tool_use` carrying no paired result block. A stricter endpoint would be
expected to reject that, so the leniency is a property of this gateway and not something a design
may rely on.

## Config corrected while measuring

The live dev config claimed `max` among muse's allowed efforts. It is rejected by the route, and the
claim was removed. luna's `none` and `max`, grok's `xhigh` and deepseek's `none` were each probed
and are honest.
