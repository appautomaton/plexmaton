# Spec — Delegation and steering

| Field | Value |
| --- | --- |
| Status | Draft — direction accepted, not implemented |
| Owns | The delegation record, its amendments, and who may write to it |
| Depends on | [Mailbox delivery](./mailbox-delivery.md); the multi-agent invariants in [`plexmaton.md`](../roadmap/plexmaton.md) |
| Proven by | Not implemented yet |

## Purpose

A delegated task has two writers: the agent that created it and the user who can steer the worker
directly. Without one authoritative record they diverge — the delegating agent keeps reporting the
task it believes it assigned while the worker does something else, and the two writers silently
overwrite each other.

This spec makes the delegation itself the authoritative record, and both writers append to it.

## Invariants

**INV-1 — One record.** A delegation has exactly one authoritative record owned by the runtime. The
delegating agent's prompt text and the user's steer message are *inputs* to that record, never
parallel copies of the task.

**INV-2 — Append-only, attributed.** Creation and every amendment are durable events carrying an
author (`Author::Agent(id)` or `Author::User`). The record is never edited in place, so the history
of what the worker was asked is always reconstructable.

**INV-3 — The delegator learns before it acts.** Every amendment is delivered to the delegating
agent as a typed event, and that delivery happens before the delegator's next turn is admitted.
A delegator must never plan against a task definition it has not seen the current version of.

**INV-4 — No silent revert.** The delegating agent cannot overwrite a user amendment. A
re-delegation that contradicts one is rejected and surfaced as an objection; it does not reach the
worker.

**INV-5 — Objections queue, never interrupt.** An objection from the delegating agent enters the
Attention queue as action-required. It never opens a modal, never steals focus, and never blocks
the user's current work.

**INV-6 — Undeliverable, never dropped.** A steer addressed to a worker that can no longer accept
it becomes `Undeliverable` with its original text retained and its reason typed. It is never
discarded, and the user is never told it was sent.

**INV-7 — The user wins, visibly.** On conflict the user's amendment is the effective instruction.
The runtime does not arbitrate the semantics of the conflict; it guarantees the conflict is
visible to both the user and the delegating agent.

## Model

```text
DelegationRecord
  id            DelegationId
  delegator     AgentId          the agent that created it
  worker        AgentId          the agent doing the work
  revisions     Vec<Revision>    non-empty; revisions[0] is the creation
  state         Open | Completed | Cancelled | Failed

Revision
  author        Author           Agent(AgentId) | User
  instruction   String
  sequence      u64              total order within the record
```

The effective instruction is the fold of every revision in order, not just the last one. A steer
that says "focus on the z-order case first" narrows the original task; it does not replace it.
Presenting only the newest revision to the worker would silently discard the delegator's framing.

### Delivery points

An amendment can arrive while the worker is mid-turn. The worker applies it at its next admitted
turn boundary, not by interrupting a turn in flight, so a tool call is never orphaned by a steer.
The amendment's delivery state is observable throughout, per
[mailbox delivery](./mailbox-delivery.md) INV-3.

## Failure modes

| Situation | Response |
| --- | --- |
| Steer sent to a worker in `Completed`, `Cancelled`, or `Failed` | `Undeliverable { reason }`, original text retained, offered for redirection or as a new task |
| Amendment arrives mid-turn | Queued to the next turn boundary; delivery state stays observable |
| Delegator re-delegates contradicting a user amendment | Rejected; raised as an objection in the Attention queue |
| Delegator objects and the user ignores it | Objection persists as action-required; the amendment stays effective |
| Worker fails after an amendment | The record keeps every revision; failure does not truncate the history |
| Two amendments race | Total order by `sequence`; both are retained, both are visible |

## Out of scope

- How a model is prompted with the record. That is a context-projection concern.
- The durable storage format and transaction model. Open research gate in
  [`plexmaton.md`](../roadmap/plexmaton.md).
- The transport that carries amendments. See [mailbox delivery](./mailbox-delivery.md).
- How the amendment renders in a transcript. See [`ui-ux.md`](../roadmap/ui-ux.md).

## Evidence

| Invariant | Proven by |
| --- | --- |
| INV-1 | Unproven — not implemented |
| INV-2 | Unproven — not implemented |
| INV-3 | Unproven — not implemented |
| INV-4 | Unproven — not implemented |
| INV-5 | Unproven — not implemented |
| INV-6 | Unproven — not implemented |
| INV-7 | Unproven — not implemented |

This table is the honest state of the spec: the direction is accepted, nothing is built. Phase 03
owns the implementation; Phase 00 owns only the surfaces that make an amendment and an objection
visible.
