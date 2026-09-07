# Multi-agent mailbox spike

| Field | Value |
| --- | --- |
| Status | Source evidence retained; COL-1–COL-5 own admission; runtime integration unproven |
| Read when | Designing Phase 03 mail admission, scheduling, recovery, or amendments |
| Contract | [Roadmap](../../roadmap.md) §Locked; LIVE-1/LIVE-3, LOOP-2/LOOP-6, JRN-4/JRN-5/JRN-7 |
| Decision gate | Crash-safe attributed mail, bounded work, responsive primary, user amendments before dependent turns |

## Corpus

Source inspection on 2026-09-07, without network, live model calls or test execution:

- Codex: `316795b3cf2a45e90d121d9f46499d4658b2645c`, clean at inspection.
- Grok Build: `72a61251fcffb464bcc687aeb5a998e5a98ec0c9`, clean at inspection.
- Plexmaton: `1393bcb0e2c0f68634a792408cf9f551342f02ff`, clean at inspection.

Reference paths below are relative to each source repository at that revision, not to a task
worktree. Local reference checkouts may advance independently. Scope is Codex `MultiAgentV2`;
legacy behavior is distinguished explicitly. The historical DSH, Kimi and Claude Code summaries
were not reverified in this pass and do not establish comparative absence claims.

## Comparison

| Dimension | Codex V2 | Grok Build |
| --- | --- | --- |
| Delegation shape | Independent session; sending and waking differ | Foreground task awaits output; 600-second budget can background it |
| Message transport | Typed `ResponseItem::AgentMessage`, with author and recipient | Owned-child `Steer` or `Queue`; completion uses internally attributed synthetic user input |
| Durability | Pending mail is memory-only; inclusion records it in rollout | Admission commits memory insertion; persistence occurs at a later safe point |
| Wake semantics | Activity-only wait; delivery and turn triggering differ | Background completion eligibility plus suppression gates inject a prompt |
| Ownership + cancellation | Session terminal events notify parent; normal V2 skips legacy watcher | Coordinator authorizes active owned child; receiver rechecks delivery capability |
| Bounds | Execution/residency limits do not bound mailbox depth | 64 active admissions, 8 per child, 32 KiB text; several transports unbounded |
| Amendments + user authority | This pass establishes no shared dual-writer delegation record | Delivery capability is not a user/delegator amendment conflict model |
| Projection to user | Typed rollout reconstruction; Attention equivalence unproven | Internal completion origin retained; provider projection becomes `role: user` |

[Source evidence](./source-evidence.md) records the pinned code paths, admission transitions,
completion gates, transport bounds and limits of these claims.

## Plexmaton fit

`plexmaton-core` has `MailId` and `ConversationEvent::MailDelivered`; the agent journal retains endpoints
and summary. Its projection validates both visible agents and emits an event, but creates no mail
context atom. `Input` has no mail variant; `LiveRuntime` owns one agent. Reusing the existing mail
event in a recipient-only journal also needs a solution for its both-endpoints validation.
`journal/projection/events.rs:169` assigns mail transcript ownership to `from`, while
`journal/compaction.rs:281` selects `to`; resolve that semantic discrepancy before reuse.
Both paths are in `crates/plexmaton-agent/src/`.

The runtime already gates effects on append acknowledgement
(`crates/plexmaton-runtime/src/runtime/transition.rs:204`), but JRN-4 makes no fsync promise.
`ContextAtomValue` has no mail variant (`crates/plexmaton-agent/src/model/context.rs:440`).
All four provider codecs need an explicit representation decision; existing parity does not prove
mail wire support.

Admission is defined by [COL-1–COL-5](../../specs/collaboration-ledger.md). Remaining runtime
integration proposals are unproven:

1. **Acceptance integration:** COL-1/COL-4 own retry and append acknowledgement. Runtime
   effects must remain behind that boundary under JRN-7; admission alone proves no recipient work.
2. **Consumption:** recipient journals retain references and exact context boundaries, not payload
   copies. Append boundary and reference atomically before model dispatch. Derive pending delivery
   on recovery; at-least-once retry has idempotent inclusion per execution lineage. This does not
   make model/tool effects exactly-once. Branch/export/delete must retain referenced logs.
   Provider adapters encode a distinct semantic mail atom.
3. **Wake:** separate `NextStep` / `NextTurn` from `QueueOnly` / `StartIfIdle`. Unclaimable step
   mail has a visible disposition. Assignment may wake; findings default to queue-only. Completion
   wakes under explicit wait/policy. Stop suppresses automatic work while preserving accepted mail.
   Notifications hint; canonical state decides.
4. **Ownership:** collaboration runtime owns topology, cancellation and bounded runners;
   runners retain loop/journal ownership. Separate stored sessions, turns and provider permits.
   Fair scheduling and reserved control capacity keep stop/shutdown usable. Terminal facts
   create recoverable result-mail debt, not detached notification tasks.
5. **Bounds:** bound payload, pending bytes, queues, spawn depth and execution independently.
   Reserve completion capacity at delegation admission; progress saturation must not lose
   completion or amendments. Generations reject callbacks from retired executions.
6. **Amendments:** task authority follows COL-3. Authenticate incoming authors and include
   amendments before the delegator's next turn, or hold it. Objections must reach Attention from
   the same item log. Runtime authentication and the pre-turn barrier remain unproven.

## Prototype and next gate

[Finite Rust model](./mailbox-model.rs), independent of Cargo; run from the repository root:

```sh
rustc --edition=2024 --test .agents/spikes/multi-agent-mailbox/mailbox-model.rs -o /tmp/plexmaton-mailbox-model
/tmp/plexmaton-mailbox-model
```

The prior prototype run recorded eight passing tests (not rerun in this source-only pass):
handoff crash cuts, recovery without retry, deduplication at capacity,
identity conflict, wake/stop, acceptance ordering, close/send order, and lost notification.
The prior mutation check made the crash-cut test fail by removing inclusion deduplication.
Assumptions: atomic acknowledged records, one recipient/lineage, two bounded mails. No filesystem,
Tokio concurrency, provider, UI or production API is exercised.

Next design gate: settle the durability contract, provider representation and dual-writer authority
together before fixing implementation order. Authority affects the foundational record schema.
Distinguish process-crash recovery from power-loss durability, and accepted/included/completed facts.
Required implementation evidence remains:

- **Storage:** real journals with crash cuts, uncertain writes, retention and branch scope.
- **Scheduling:** A/B scripted providers, saturation and joined shutdown; responsive primary.
  Decide parent-turn stop versus subtree shutdown and reserved capacity.
- **Amendments:** race both authors and turn opening, including context-budget failure; prove
  attribution, user precedence and the pre-turn barrier.
- **Projection:** all four provider codecs preserve provenance; resume adds no synthetic user
  turn; inbox and Attention share source items. Review three widths when UI changes.

[Phase 03](../../phases/phase-03-collaboration.md) has delivered durable admission; scheduling,
provider projection and the pre-turn authority barrier remain separate implementation gates.
