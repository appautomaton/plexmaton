# Plan — Phase 04 stage 33, provider-side tools

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) stage 33 |
| Contract | PRV-1/PRV-3/PRV-5/PRV-6, [tool-admission](../specs/tool-admission.md), [ui-ux](../ui-ux.md) transcript grammar |
| Evidence | [Provider-side tools spike](../spikes/provider-side-tools/README.md) |
| Status | Slices 1–5 implemented and locally verified; slice 6 open for the spike's promotion and this plan's deletion |

## Outcome

A model may declare the hosted tools its route accepts, and a Session using that model reaches
provider-hosted web search without the transcript losing sight of what was searched. The harness
forwards a declaration and renders what comes back. It never runs the search, never infers the
capability from a model's name, and never silently absorbs a block it cannot show.

Reversal recorded deliberately: PRV-5 currently states that a provider-side block is refused. That
sentence was true when written and this stage rewrites it, along with the rejected alternative
beside it, rather than leaving a second reading in the corpus.

## Slices

1. **Declaration — implemented.** A model entry gains a hosted-tool subset, validated the way
   `allowed_reasoning_efforts` already is: present or absent, nonempty and unique when present, and
   encodable by that model's dialect. A declaration a dialect cannot spell fails at config load,
   before any network work. PRV-6 gains the field; nothing infers it from a provider or model name.

2. **Request encoding — implemented.** Each dialect spells the declared capability and owns that spelling alone.
   Messages emits `{"type":"web_search_20250305","name":"web_search"}`; Responses emits
   `{"type":"web_search"}`; Chat Completions emits `web_search_options`, the field OpenAI's own
   search models take; GenerateContent's `google_search` tool is spelled once its facade has been
   measured. Slice 1's gate is the tool name, not the dialect: a declared tool a dialect has no
   spelling for fails at config load. A route may accept a spelling and ignore it, so a declaration
   is the owner's claim about their route, verified once rather than trusted. No request carries a
   hosted tool the model did not declare.

3. **Decode — implemented.** The Responses decoder accepts a `web_search_call` output item on both `added` and
   `done`, carrying the action the provider took: a query, an opened page, or a find within one.
   An action that arrives without a query is carried as it arrived, not refused. One semantic
   event, shaped so a Messages decoder can emit the same event later without a second one, because
   the spike found the two dialects differ only in wrapper. The Messages `server_tool_use` block
   keeps PRV-5's refusal in this stage, because no Messages route is in daily use to prove a decoder
   against. Usage stops failing a step when a provider reports non-zero hosted-tool counts, and
   accounts them instead.

4. **Transcript — implemented.** The call reaches the workspace where the provider placed it, as a
   running row that finishes once, in the tool row's grammar and coloured for who ran it (ui-ux
   §transcript grammar, ENT-2); a reopened conversation shows the same rows in the same places, and
   `scripts/smoke-server-tool.py` drives the burst one live gateway produced through the binary to
   prove it. The row shows what its route returned and claims nothing further; the
   [spike](../spikes/provider-side-tools/meta-route.md) records what each measured route gives. The
   user chose the grammar and colour from rendered candidates and reviewed the real frames at three
   widths on 2026-09-21.

5. **Replay — implemented.** A hosted-tool item round-trips when the conversation continues, under PRV-3's
   existing sidecar rules, with its status and without its provider-assigned id, which the route in
   daily use rejects and Meta documents as optional. The test says so, and a live follow-up turn
   proved it.

6. **Corpus.** PRV-5's refusal and its rejected alternative are rewritten to what the code now
   does. PRV-6 gains the declaration. The evidence table gains a row per invariant. The spike is
   promoted where it now holds a decision, and the parts that were only reconnaissance are dropped.

## Order and why

Declaration precedes encoding because a spelling with nothing to spell cannot be tested. Encoding
precedes decode because a fixture is cheaper to trust when the request that produced it is ours.
Decode precedes the transcript because a row has nothing to render until an event exists, and the
contract owns the row's shape rather than the adapter. Replay is last because it is the only slice
that needs two turns, and because the arm it picks is informed by what the row had to show.

## Open, to settle inside the stage

- Where does hosted-tool spend belong in the cost surface? It is provider-side work the owner did
  not select a model for, and the usage counter names requests rather than tokens.

## Deliberately not in this plan

Running any search locally. A client tool that issues its own isolated model request, which is pi's
shape and would need tools to reach a provider route. Hosted tools other than search, which no
measured route offered. Recovering findings a route does not return. Per-call approval, because the
fence position already holds that configuring a provider authorises the call this rides on.
Decoding `server_tool_use` on Messages, which keeps its refusal until a Messages route is in daily
use. The owner's gateway configuration, which changes nothing about what this stage encodes or
decodes.
