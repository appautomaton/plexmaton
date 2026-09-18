# Spec — Attention queue

| Field | Value |
| --- | --- |
| Status | Implemented for both routes: primary approvals answer inline; live and passively reopened child requests use canonical collaboration references and the child's roster row. A process-dead request is visible but inert; fresh work creates a new request |
| Owns | Pending request presentation, main-agent approval sequencing, and where a background request is announced |
| Depends on | The attention rules in [`ui-ux.md`](../ui-ux.md) §attention management; the focused-surface grammar in [interaction-routing](./interaction-routing.md) INV-10 |
| Proven by | `plexmaton-tui::state::attention`, `::content::roster` and `::workspace` tests |

## Invariants

**ATT-1 — A *background* request costs the user nothing.** A request from an agent whose
conversation the user is not in moves no keyboard focus, changes no selected agent, opens no
dismissible surface, moves no text cursor, and disturbs no draft. It is announced on that agent's
roster row and nowhere else: the row takes the action-required color, sorts above the agents that
are only working, and says what is wanted. When Narrow shows a child and hides the primary activity
line, its collapsed conversation-top handle carries the aggregate unanswered count and opens the
full-region roster. Both presentations use already reserved space, so the announcement costs the
conversation no row.

Rejected: a separate Attention band. It was a third home for a fact the roster and the raising
conversation already carried, it charged rows to the terminals with fewest, and it needed its own
cursor, its own focus stop and its own filtered view of the queue to say what one row now says.

Primary-agent requests are never announced on the roster. A pending primary approval opens inside
its conversation; later arrivals cannot replace that card or its chosen decision. Producer
resolution advances to the oldest remaining approval, with Deny selected. Esc returns to the
composer and leaves the card visible; Tab/click can return to it. The card is a non-blocking input,
so reading history and composing remain possible. A background card is a modal and closes on Esc;
its request is still outstanding, so the roster still names it and entering the agent reopens it.
Pointer decisions require a matching, unchanged request and press/release; drag, focus loss, resize
or replacement cancels activation. The card separates the operation, the policy reason and the
choices; submitting disables duplicate decisions until producer confirmation, and refusals stay
visible.

**ATT-2 — Going to a request is a keypress the user made.** `Enter` on a roster row opens that
agent's window, and opening an agent that is asking something is going to its request. No producer
path reaches that verb: nothing a background agent does selects an agent or opens a window.

**ATT-3 — Acknowledging is not resolving.** Going to a request marks it seen and leaves it queued,
because it is still outstanding. Only `AttentionResolved` from the owning loop clears it; closing
an approval surface or sending a decision does not optimistically remove loop-owned state. A seen
request keeps its roster row and stops competing for attention: the row holds the muted role rather
than disappearing, or a user who closed a card would have no way back to it. An agent that asks
again arrives unseen.

## Model

```text
ConversationEvent::AttentionRequested ──▶ AttentionQueue (arrival order, coalesced by AttentionId)
                                            │
                                            ├──▶ the asking agent's roster row (ATT-1)
                                            │
   user presses Enter on that row ──────────┴──▶ acknowledge + open its card where the agent is
                                            │
ConversationEvent::AttentionResolved ───────┴──▶ remove the exact request
```

A delegated producer commits the complete request or resolution in its own journal first. The
collaboration log then admits only its authenticated endpoint and `AttentionId`; only that joined
pair may cross into the root projection. The log reference never becomes model context and does
not copy the request payload or pending-state machine. Live approval routes are process-local,
bound to the exact child runner generation and consumed through its owner. Passive replay reads the
same validated log/journal prefix without constructing a runner; a request with no matching log
reference stays inert, and replay issues no decision route. Process recovery may preserve the old
request's presentation, but its approval is cancelled and a decision returns `NotPending`; only a
new explicit submission can create another request under current policy. Graceful shutdown drains a producer's
final request and resolution events into the collaboration log before closing its writer, so a
resolved request cannot return on the next passive reopen.

| Fact | Value |
| --- | --- |
| Coalescing | By `AttentionId`: a repeat replaces its entry in place and keeps its position, so an agent asking twice is one item |
| Place | The asking agent's row in the roster panel, which is already a focus stop and a pointer target. No surface is registered for the queue itself |
| Which request a row shows | An approval outranks a clarification, because one agent is blocked and the other is not; within a kind it is arrival order |
| Ordering | Failure, then an unanswered request, then everything else, with a ruled break between what is addressed to the user and what is not |
| Counts | The primary activity line ends with `( !n )`; when Narrow shows a child instead, its collapsed Agents handle carries `!n`. Both use existing chrome, cost no row and add no focus stop. Rejected: the count on the open roster's title, beside rows already carrying each agent's state |

## Failure modes

| Situation | Response |
| --- | --- |
| A request from an agent not in the roster | Rejected at ingest as an unknown agent, which reaches the notice log |
| `Enter` on a roster row whose agent is asking nothing | The window opens; nothing is acknowledged |
| A resolution names another agent's request | Rejected as an ownership mismatch and shown in the notice log |
| An agent with more than one request | The row names the one that outranks; entering again goes to the next |
| Every queued request belongs to the primary | No roster row says anything: the card is already on screen, and the pill counts nothing |
| A terminal too narrow for the roster's column | The full-region roster opens from the conversation-top handle; closed, that existing chrome still counts what is unanswered |

## Evidence

[Named proofs](../evidence/attention.md), one row an invariant.
