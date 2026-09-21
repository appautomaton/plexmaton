# Provider-side tools spike

| Field | Value |
| --- | --- |
| Status | Comparison and live measurement retained as evidence; the design is decided in [stage 33](../../plans/phase-04-stage-33-provider-side-tools.md) |
| Read when | Adding web search, or any tool a provider runs on its own side rather than handing to this harness |
| Question | Can a Session reach provider-hosted search without the harness losing sight of what was searched? |
| Contract | [provider-adapter](../../specs/provider-adapter.md) PRV-1/PRV-3/PRV-5; [tool-admission](../../specs/tool-admission.md); [permission-policy](../../specs/permission-policy.md) PER-2 |

## Why this exists

`provider-adapter.md` PRV-5 records that a `server_tool_use` block is refused, and why. It does not
say whether that refusal should stand. A provider that runs a search on its own side is the one
category the fence position does not cover: not something we execute, not something we hand to a
subprocess, but something the provider performs inside a call the owner already authorised.

## Corpus

Local source read 2026-09-20; no reference suite was run. Plexmaton base `c7bdf0a`, branch
`feat/provider-side-tools`.

| Directory | Revision | Source root for references below |
| --- | --- | --- |
| `codex` | `9f70e348e0227980de97e361cce830236fb18317` | `codex-rs` |
| `pi-arcweld` | `26cb4bf5cf7e7c4bb5fe948863264ed3cf899048` | `extensions/claude-web-search` |
| `grok-build` | `72a61251fcffb464bcc687aeb5a998e5a98ec0c9` | `crates/codegen/xai-grok-pager/src` |
| `claude-code` | `2.1.88` mapped | `cc/2.1.88` |
| `kimi-code` | `f12d59e089e2531a33fbca30b26ffeabd5862b45` | — |

## Two shapes, and they are not variations of each other

**Codex makes it a first-class protocol item.** `ResponseItem::WebSearchCall` carries id, status and
a typed `action`, and `event_mapping.rs:228` turns it into `TurnItem::WebSearch` for the transcript.
The variant is matched in rollout normalisation, turn timing, remote compaction, image preparation
and persisted state — so the cost is not one enum arm but every site that already matches the enum.
`WebSearchItem.results` exists and is set to `None` on this path, because the inline Responses call
returns the query and not the findings; only a separate standalone search populates it, and its
comment keeps those results as opaque JSON at the transport boundary so new result shapes need no
release.

**pi makes it an ordinary client tool.** `extensions/claude-web-search` registers a `WebSearch`
tool with plain `query` / `allowed_domains` / `blocked_domains` parameters. Its execution issues an
**isolated** Messages request carrying the hosted tool, and returns a synthesis plus source URLs as
an ordinary tool result. `payload.ts` refuses a payload that is not exactly one user message before
continuation history, which is what keeps that request isolated. `getHostedWebSearchRoute` gates the
whole thing on the active model's provider and API, and `syncWebSearchAvailability` adds or removes
the tool from the active set as the model changes.

Its consequence is real and worth recording: **the main conversation never contains a
`server_tool_use` block.** No new content-block type, no new semantic event, no replay hole, and the
search takes the ordinary admission, permission and transcript path every other tool takes.

Rejected all the same, for two reasons that only appear once it is drawn against this codebase. A
tool that issues its own model request needs to reach a provider route, and native tools here reach
a workspace, ripgrep and a command — giving them a route is a larger boundary change than the one it
saves. And it spends a second model call for every search, chosen by the harness rather than by the
owner, which is exactly the accounting question the owner has no surface to answer. The owner's
instruction settles the rest: the declaration goes to the server side, and the harness renders what
returns.

`grok-build` sits nearer Codex, handling search inside the pager's scrollback block types.
`claude-code`'s only server-tool surface in readable source is the usage counter shape
(`web_search_requests`, `web_fetch_requests`) that this harness already parses.

## Configuration declares, the dialect spells

PRV-6 already decides the half that matters: a model owns its declared capabilities, and
**selection never uses fuzzy names or URL inference**. A harness that switches hosted search on
because a model id looks familiar has put authority in a name, which is the thing that invariant
exists to refuse.

`allowed_reasoning_efforts` is the precedent to copy, not a new pattern to invent. The model
declares a subset, the declaration must be encodable by that model's dialect, and an invalid one
fails before any network work. A hosted-tool declaration is the same shape:

```toml
[providers.local.models.muse]
api = "openai_responses"
id = "muse-spark-1.3"
server_tools = ["web_search"]
```

Config names the capability. The adapter owns its spelling, because the spelling is dialect
property and nothing else: Messages wants `{"type":"web_search_20250305","name":"web_search"}` and
Responses wants `{"type":"web_search"}` or its `web_search_preview` alias. That is the compatibility
work, and it belongs behind the declaration rather than in front of it. A tool declared on a dialect
that cannot encode it fails at config load, the way an unencodable effort already does.

The failure mode of a declaration is known and cheap: it can be wrong. The live config claimed
`max` among muse's efforts and the route refused it. The fix was to correct the declaration, never
to add an inference that would have guessed around it.

Rejected: gating on the model's identity, which is how every harness read here decides it. pi's
`getHostedWebSearchRoute` matches provider names and `id.startsWith("claude-")`, so every new route
and every renamed model is a code change, and a model the owner knows supports search cannot be
told so. A declaration costs one line in a file the owner already edits and answers both.

## What this costs in Plexmaton

The chosen shape is Codex's, narrowed by the declaration. Cost lands in the provider adapter and the
transcript, and Codex's own use sites measure the blast radius honestly: a response variant is
matched wherever the enum is already matched, so compaction, persisted state, timing and replay each
have to say what they do with it. Plexmaton's equivalent is one new semantic event, and the sites
that match its event vocabulary are the ones to count before writing any of it.

What the declaration buys is that none of this reaches a model that did not ask for it. A route
with no hosted tools encodes nothing new and decodes nothing new, so the existing four dialects keep
their current behaviour exactly.

[Stage 33](../../plans/phase-04-stage-33-provider-side-tools.md) owns the order.

## Open

- Is strict cloaking what strips search results on the local gateway, or does the upstream never
  send them? `claude_executor_cloaking.go` handles `server_tool_use` and is the place to look.
- Codex's standalone path populates `results`. Worth reading before assuming the findings are
  always unavailable.
- Where does hosted-tool spend belong? The usage counter names requests rather than tokens, and
  no surface shows provider-side work today.

## Limits

Reference behaviour is read from source, not observed at run time. The gateway measurements in
[gateway evidence](./gateway-evidence.md) are live and reproducible, and they describe one route on
one machine, not Anthropic's or Meta's own service.
