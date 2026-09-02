# Spec — Mailbox delivery

| Field | Value |
| --- | --- |
| Status | Draft — direction accepted, not implemented |
| Owns | The typed item log, its delivery states, and the projections over it |
| Depends on | The durable-store research gate in [`plexmaton.md`](../roadmap/plexmaton.md) |
| Used by | [Delegation and steering](./delegation-and-steering.md) |
| Proven by | Not implemented yet |

## Purpose

Everything that moves between sessions — mail, attention requests, delegation amendments,
objections — travels through one log. If the inbox and the Attention queue were separate stores
they would need reconciliation code, and reconciliation code is where hidden state lives.

This spec makes both of them projections over a single append-only log, so they cannot diverge.

## Invariants

**INV-1 — One log.** Session events, mail, attention requests, and delegation records share one
storage boundary. There is no second store to synchronise against.

**INV-2 — Typed kinds.** Every item carries a typed kind. There is no generic "message" that the
interface has to inspect textually to decide how to render or route it.

**INV-3 — Observable delivery.** Every item is always in exactly one observable state:
`Queued → Delivered → Acknowledged`, or terminally `Undeliverable`. No item is ever in a state the
user or a test cannot inspect.

**INV-4 — Acknowledgement is not resolution.** Acknowledging an item is idempotent and does not
resolve what it is about. Dismissing the badge on an approval request does not grant the approval.

**INV-5 — Nothing is dropped silently.** An item that cannot be delivered becomes `Undeliverable`
with a typed reason and its payload retained. Discarding an item without a durable record of the
discard is forbidden.

**INV-6 — Total order per recipient.** Items delivered to one recipient have a total order, so
"the delegator saw the amendment before the worker's result" is a fact rather than a race.

**INV-7 — Projections own nothing.** The inbox and the Attention queue are derived views over the
log. Deleting either view loses no data, and neither can hold an item the log does not.

**INV-8 — Coalescing preserves the log.** Repeated updates from one sender may collapse into a
single attention item in the *view*, but every underlying item stays in the log. Coalescing is a
presentation rule, never a write.

## Model

```text
Item
  id          ItemId
  kind        Mail | AttentionRequest | DelegationAmendment | Objection
  from        Author            Agent(AgentId) | User | Runtime
  to          AgentId | User
  sequence    u64               total order per recipient
  payload     <per kind>
  delivery    Queued | Delivered | Acknowledged | Undeliverable { reason }
```

### Projections

| View | Derivation |
| --- | --- |
| Inbox for `X` | items where `to == X`, ordered by `sequence` |
| Attention queue | items where `kind ∈ {AttentionRequest, Objection}` and not resolved, ordered by `sequence` |
| Undelivered | items where `delivery == Undeliverable`, offered for redirection |

The Attention queue is therefore not a list the runtime maintains. It is a filter, which is what
makes INV-7 structural rather than a discipline someone has to remember.

## Failure modes

| Situation | Response |
| --- | --- |
| Recipient is completed, cancelled, or failed | `Undeliverable { reason }`; payload retained and offered for redirection |
| Recipient is mid-turn | Stays `Queued` until the next turn boundary; the state remains visible |
| Delivery is attempted twice | Idempotent by `ItemId`; the second attempt is a no-op, not a duplicate |
| Acknowledgement arrives twice | Idempotent; acknowledging is not a counter |
| Sender floods one recipient | The view coalesces; the log does not, per INV-8 |
| Store is unavailable | Typed, visible degradation. Queued items are not reported as delivered |

## Out of scope

- The storage engine and transaction model. Open research gate in
  [`plexmaton.md`](../roadmap/plexmaton.md); the decision belongs to Phase 02.
- Retention and compaction of the log.
- How the Attention queue is presented and ordered on screen. See
  [`ui-ux.md`](../roadmap/ui-ux.md).
- Agent-to-agent semantics of a particular item kind. See the spec that owns that kind.

## Evidence

| Invariant | Proven by |
| --- | --- |
| INV-1 … INV-8 | Unproven — not implemented |

Phase 03 owns the implementation. The workspace already has the surfaces that make queue
membership, delivery state, and undeliverable items visible.
