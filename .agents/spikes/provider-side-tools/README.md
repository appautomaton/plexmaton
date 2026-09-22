# Provider-side tools spike

| Field | Value |
| --- | --- |
| Status | Implemented as phase 04 stage 33; PRV-5, PRV-6 and ENT-2 own the behaviour. Live measurements retained as evidence for the next hosted tool |
| Read when | Adding web search, or any tool a provider runs on its own side rather than handing to this harness |
| Question | Can a Session reach provider-hosted search without the harness losing sight of what was searched? |
| Contract | [provider-adapter](../../specs/provider-adapter.md) PRV-1/PRV-3/PRV-5; [tool-admission](../../specs/tool-admission.md); [permission-policy](../../specs/permission-policy.md) PER-2 |

## Why this exists

A provider that runs a search on its own side is the one category the fence position does not
cover: not something we execute, not something we hand to a subprocess, but something the provider
performs inside a call the owner already authorised. PRV-5 once refused the block outright; this
spike is why it now carries the call and types its outcome.

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

## Two shapes, and the one chosen

Codex carries a provider-run search as a first-class protocol item, matched at every site that
already matches its response enum. pi wraps it as an ordinary client tool that issues its own
isolated Messages request, so its main conversation never holds a `server_tool_use` block. Codex's
shape was taken, narrowed by a declaration. pi's was rejected because a tool that issues its own
model request needs a provider route that native tools here do not have, and because it spends a
second model call per search that the harness, not the owner, chose. grok-build sits nearer Codex;
claude-code's only readable server-tool surface is the usage counter this harness already parses.

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

## Open

- Is strict cloaking what strips search results on the local gateway, or does the upstream never
  send them? `claude_executor_cloaking.go` handles `server_tool_use` and is the place to look.
- Codex's standalone path populates `results`. Worth reading before assuming the findings are
  always unavailable.

## Limits

Reference behaviour is read from source, not observed at run time. The gateway measurements in
[gateway evidence](./gateway-evidence.md) are live and reproducible, and they describe one route on
one machine, not Anthropic's or Meta's own service.
