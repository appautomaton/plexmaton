# Multi-agent mailbox spike

| Field | Value |
| --- | --- |
| Status | Current source evidence retained; COL-1–COL-5 and CIN-1–CIN-4 own production mechanisms |
| Read when | Designing Phase 03 mail admission, scheduling, recovery or control handoff |
| Contract | [Roadmap](../../roadmap.md) §Locked; LIVE-1/LIVE-3, LOOP-2/LOOP-6, JRN-4/JRN-5/JRN-7 |
| Decision gate | Crash-safe attributed mail, bounded work, responsive primary and exclusive control before execution |

## Corpus

Source inspection on 2026-09-12 used freshly fetched remote refs without checkout changes, live
model calls or reference-harness execution:

- Codex `b979d4f1f04538ba5a5fcc434d499c007bfe1b8c` (`origin/main`).
- Grok Build `37949780c144e37df692e3d669051a21fec24f20` (`origin/main`).
- Kimi Code `ee2cac102b835fcd7adb3d4b9bc3d62b0b71cdfd` (`origin/main`).
- DeepSeek Harness `c291e7961a515f6d7af9304e7fd1d257929aef26` (`origin/master`).
- Plexmaton base `e5e8e0b1ce9f26d109c868bcf7ef63ba46773e4a`; exclusive-control evidence belongs to its uncommitted task worktree.

Reference paths in [source evidence](./source-evidence.md) are relative to each repository at the
pinned remote ref. Codex scope is `MultiAgentV2`; legacy behavior is named explicitly.

## Comparison

| Concern | Best evidence | Limit that Plexmaton must not inherit |
| --- | --- | --- |
| Independent child sessions and typed provenance | Codex V2 | Pending delivery is process-local; no durable exclusive handoff |
| Coordinator-owned admission, cancellation and finalization | Grok Build | Some completion projection becomes synthetic user-role input |
| Retry invalidation below a turn | Kimi Code | Attempt reset is not durable Conversation authority |
| Cancellation/recovery race coverage | DeepSeek Harness | Inbox/abort state does not establish Plexmaton's JSONL control record |
| Durable controller and handoff | Plexmaton COL-3 | The attached runtime gate does not supply the asynchronous owner or authenticated product ingress |

Grok is the primary execution-ownership reference; Codex is the primary session/provenance
reference. Neither supplies Plexmaton's durable authority. Kimi and DeepSeek contribute targeted
late-callback, cancellation and recovery cases rather than product semantics.

## Decision

Delegation remains asynchronous. The delegating turn does not block on child completion, and
cross-Conversation content remains typed, attributed mail rather than a synthesized user message.
Bulk evidence stays in the child or an artifact; mail carries a bounded summary and pointers.

A delegated Conversation has one controller across any number of child turns. Main may update its
task and open later turns while the child is idle. The user may inspect and stop it, but direct input
opens only after acknowledged, one-way release. Idle, completion, process exit and surface closure
do not infer transfer. Release changes neither history nor tool capability.

The source comparison rejected the prior dual-writer task model. User/Main precedence, objections
and concurrent amendment races add a second arbitration mechanism when explicit handoff can give
each input one owner. Serialized authors remain evidence, never credentials; runtime ingress must
authenticate Main before constructing an event.

## Plexmaton fit

[COL-1–COL-5](../../specs/collaboration-ledger.md) own the bounded JSONL item log, Main-authored task
updates, one-way durable release, exact retry and shared writer lease. A `TurnAdmission` remains
inspectable history; the file/control owner issues one non-cloneable reservation/permit under
current Main control. The narrow runtime accepts a previously held reservation plus an exact ticket,
then retains the bound permit through session acknowledgement and owned execution. Any unknown
collaboration write freezes authority until reopen, and a live reservation or permit retains the
physical writer lock. The future product owner must acquire the reservation before accepting Main
input.

[CIN-1–CIN-4](../../specs/collaboration-inclusion.md) retain canonical admission references and a
distinct collaboration context atom. All provider codecs still refuse unsupported representation
rather than fabricating user input. Session inclusion and request authorization remain separate
acknowledgement barriers; no model/tool effect is exactly-once merely because admission is durable.

The earlier [finite model](./mailbox-model.rs) remains bounded prototype evidence only. It exercises
eight handoff, recovery, capacity, wake/stop and lost-notification cases without files, Tokio,
providers, UI or production APIs.

## Next gate

Attached child runtimes now refuse direct user input before mutation and retain a bound Main permit
through inclusion, request authorization, owned work, cancellation and join. The next gate is the
bounded asynchronous collaboration owner that authenticates ingress and acquires the reservation
before accepting Main input. Later stages own child bootstrap, bounded scheduling, provider
representation and Inbox/Attention projection. JRN-4 promises process-death recovery, not
power-loss durability.
