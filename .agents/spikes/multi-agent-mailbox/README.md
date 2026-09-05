# Multi-agent mailbox spike

| Field | Value |
| --- | --- |
| Status | Source comparison and finite model complete; production design unproven |
| Read when | Designing Phase 03 mail admission, scheduling, recovery, or amendments |
| Contract | [Roadmap](../../roadmap.md) §Locked; LIVE-1/LIVE-3, LOOP-2/LOOP-6, JRN-4/JRN-5/JRN-7 |
| Decision gate | Crash-safe attributed mail, bounded work, responsive primary, user amendments before dependent turns |

## Corpus

Inspected local source on 2026-09-04, without network or live model calls:

- Codex: `316795b3cf2a45e90d121d9f46499d4658b2645c`, clean checkout.
- Grok Build: `72a61251fcffb464bcc687aeb5a998e5a98ec0c9`, clean checkout.
- Plexmaton: base `11f5d6ee70fb2832417f1991948a7ea35e3cf7bf`, with existing session/UI
  modifications. Findings describe the working tree.

Scope: local Codex `MultiAgentV2`, not legacy `send_input` or installed binaries.
Reference tests were inspected, not run.

Entrypoints: Codex [dispatch][c-message], [queue][c-queue], [control][c-control];
Grok [coordinator][g-coordinator], [admission][g-message], [completion][g-completion].

## Comparison

| Dimension | Codex v2 | Grok Build |
| --- | --- | --- |
| Delegation | Independent session; sending and waking differ | Task defaults to awaiting result; background returns after registration; deadline can auto-background |
| Messaging | Typed mail; delivery respects step/turn phases | Root tool targets owned active descendants with `Steer` or later-turn `Queue`, not general peer mail |
| Waiting | `wait_agent` reports activity/timeout, not child output | Actor owns result waiters/deadlines |
| Completion | Detached watcher sends direct-parent mail, `trigger_turn=false` | Gate checks cancellation, backgrounding, waiter delivery, kill, goal loop and parent channel |
| Bounds | Execution/residency limits separate; mailbox enqueue has no explicit bound | Active messages: 64 ingress, 8 per child, 32 KiB; completed cache: 1024; other channels include unbounded ones |
| Recovery | Consumed mail enters rollout; pending enqueue is in memory | Child metadata/output and attempt recovery; orphan repair for terminal presentation |

Codex `session/handlers.rs::inter_agent_communication` queues, then considers starting pending
work for triggering mail **or outstanding durable sleep**. Queue-only does not prevent a sleeping
task from resuming. `context/inter_agent_message.rs` and its completion counterpart render
attributed `assistant` fragments. `session/mod.rs::record_inter_agent_communication` persists
response items and metadata when included in history; this does not prove pending-mail durability.
The detached completion watcher discards send errors.

Grok `task/active_message.rs` uses `Open → Claimed → Committed` admission; revocation succeeds
before claim. Protected insertion cannot cross an await. Timeout after claim can yield
`AdmissionUncertain`; finalization drains admissions and records uncertainty. `Accepted` means
admitted, not model-consumed. The shell verifies `OwnedActiveDescendantGrant`, identity, content
and operation before forwarding `ParentAgentMessage`. Completion auto-wake injects a prompt.


## Plexmaton fit

`plexmaton-core` has `MailId` and `SessionEvent::MailDelivered`; the agent journal retains endpoints
and summary. Its projection validates both visible agents and emits an event, but creates no mail
context atom. `Input` has no mail variant; `LiveRuntime` owns one agent. Reusing the existing mail
event in a recipient-only journal also needs a solution for its both-endpoints validation.

```text
Agent effect / user intent
          │
          ▼
CollaborationRuntime ── append/ack ── canonical cross-session item log
          │                                 │
          │ bounded owned session runners   ├─ inbox / Attention projections
          ▼                                 │
recipient LiveRuntime ◀─ pending MailId ─────┘
          │
          └─ append boundary + MailId reference ── ack ── model request
```

Proposed rules, unproven in production:

1. **Acceptance:** one log owns each envelope. `Accepted(MailId)` follows append ack; inclusion
   and completion are different facts. Retry a sender-scoped identity; refuse conflicting content.
   Unknown append freezes effects until reopen; preserve JRN-4.
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
6. **Amendments:** one revisioned delegation record retains authorship. Refuse stale delegator
   writes and preserve user authority; revision checks alone do not implement precedence. Include
   amendments before the delegator's next turn, or hold it. Objections share the item log and
   Attention projection. No inspected source proves this requirement.

## Prototype and next gate

[Finite Rust model](./mailbox-model.rs), independent of Cargo; run from the repository root:

```sh
rustc --edition=2024 --test .agents/spikes/multi-agent-mailbox/mailbox-model.rs -o /tmp/plexmaton-mailbox-model
/tmp/plexmaton-mailbox-model
```

Eight tests passed: handoff crash cuts, recovery without retry, deduplication at capacity,
identity conflict, wake/stop, acceptance ordering, close/send order, and lost notification.
Removing inclusion deduplication makes the crash-cut test fail (mutation check).
Assumptions: atomic acknowledged records, one recipient/lineage, two bounded mails. No filesystem,
Tokio concurrency, provider, UI or production API is exercised.

Next implementation evidence, ordered by dependency:

- **Storage:** real journals with crash cuts, uncertain writes, retention and branch scope.
- **Scheduling:** A/B scripted providers, saturation and joined shutdown; responsive primary.
  Decide parent-turn stop versus subtree shutdown and reserved capacity.
- **Amendments:** race both authors and turn opening, including context-budget failure; prove
  attribution, user precedence and the pre-turn barrier.
- **Projection:** both codecs preserve provenance; resume adds no synthetic user turn; inbox and
  Attention share source items. Review three widths when UI changes.

Phase 03 stays unopened; these questions should slice its implementation.

[c-message]: ../../../../codex/codex-rs/core/src/tools/handlers/multi_agents_v2/message_tool.rs
[c-queue]: ../../../../codex/codex-rs/core/src/session/input_queue.rs
[c-control]: ../../../../codex/codex-rs/core/src/agent/control.rs
[g-coordinator]: ../../../../grok-build/crates/codegen/xai-grok-tools/src/implementations/grok_build/task/coordinator.rs
[g-message]: ../../../../grok-build/crates/codegen/xai-grok-tools/src/implementations/grok_build/task/coordinator/active_message.rs
[g-completion]: ../../../../grok-build/crates/codegen/xai-grok-shell/src/agent/subagent/spawn.rs
