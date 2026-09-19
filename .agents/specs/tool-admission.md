# Spec — Tool admission and approval

| Field | Value |
| --- | --- |
| Status | Implemented; process-dead approvals are cancelled and new submissions use fresh admission/current policy |
| Owns | How an untrusted model tool request becomes an admitted call, how policy decides it, and how an approval decision names it |
| Depends on | [agent-loop](./agent-loop.md) LOOP-2 through LOOP-5; [attention](./attention.md) ATT-1 and ATT-3 |
| Proven by | `plexmaton-agent::{admission,tools,turn}`, native-tool admission, `plexmaton-runtime::runtime::tests::tools`, and `plexmaton-tui::{approval,workspace,frames}` tests |

## Invariants

**APV-1 — Authority starts at admission.** The model supplies an untrusted tool name and raw
arguments. The trusted catalog validates and canonicalizes them through an explicit loop effect
carrying a non-cloneable, loop-issued `AdmissionRequest`. Consuming it produces one immutable
admitted call or a typed refusal before policy or execution can run; neither the model, a
presentation adapter, nor a holder of an admitted call can construct another. Raw provider
arguments stop at 64 KiB; canonical state has a separate 1 KiB structural reserve so trusted
normalization does not widen the wire boundary.

**APV-2 — Policy reads trusted semantic facts.** An admitted call carries definition identity and
revision, normalized arguments, typed capabilities, bounded decision detail and a bounded canonical
invocation. Policy returns `Allow`, `RequireApproval`, `Forbidden`, or typed source `Unavailable`; tool
names, display labels and prompt prose are never authority. The default capability fallback is extended by [permission-policy](./permission-policy.md)
PER-2–PER-4; remembered scopes use catalog-issued permission subjects.

**APV-3 — Approval grants permission, not validity.** `AllowOnce` authorizes only the admitted call
the request pins. It cannot override an admission refusal, relax workspace confinement, satisfy an
integrity precondition such as read-before-edit, or replace executor-boundary enforcement.

**APV-4 — A decision names one stable pending call.** A pending record binds an `ApprovalId` to its
`AgentId`, `TurnId`, `ToolCallId` and admitted call. The UI returns that ID with `AllowOnce`, `Deny`, or a producer-issued remembered offer and
lifetime; PER-5 owns preparation. An absent, stale or mismatched ID is a typed non-decision. IDs
bind the Conversation, head, turn and journal position as well as the agent/call.

**APV-5 — Waiting is per call.** A protected call may wait while admitted siblings run; the model
does not receive the batch until every slot has paid its result debt, assembled in model order
(LOOP-2, LOOP-3). Denial and cancellation finish the exact slot with typed results rather than
discarding its payload.

**APV-6 — Waiting has no hidden waiter and no approval timeout.** The pending record is inspectable
turn state (LOOP-4, LOOP-5), not a task, callback, sender or blocking thread. Interrupt, turn
cancellation, shutdown and process recovery explicitly cancel it. A restored approval cannot
continue and no recorded request or decision becomes authority. If the user still wants the work,
a new explicit submission starts a new model turn; every new call runs fresh admission and current
policy. Rejected: reconstructing an executable call from the old presentation or approval record.

## Model

```text
Requested
    │
    ▼
AwaitingAdmission ── refusal ─────────────────────────▶ Finished(refused)
    │ admitted call
    ▼
Policy ── Forbidden ──────────────────────────────────▶ Finished(forbidden)
    ├──── Allow ──────────────────────────────────────▶ Running ──▶ Finished(result)
    └──── RequireApproval ──▶ AwaitingApproval
                                  ├─ AllowOnce ───────▶ Running
                                  ├─ Deny ────────────▶ Finished(denied)
                                  └─ cancel/shutdown ─▶ Finished(cancelled)
```

Each tool call owns one slot. The batch owns their model order; completion order is not a second
ordering.

## Extension boundaries

| Boundary | Current | Later |
| --- | --- | --- |
| Tool catalog | Native read, search, create, edit, command and skill definitions declare schemas, capabilities and bounded details | Optional MCP integration must use the same admission boundary |
| Approval policy | Revisioned Session snapshots, capability fallback and durable project rules/grants (PER-1–PER-10/PGR-1–PGR-5); process recovery cancels old approvals before new submitted work is admitted | Future transports retain the same boundary |
| Decision vocabulary | `AllowOnce`, `Deny`, `AllowAndRemember` with producer-issued offers and project preparation tickets (PER-5/PER-6); the UI never supplies a matcher | Future transports retain producer-owned offers |
| Presentation | One revisioned transcript entry follows the call lifecycle; Attention projects the pending decision separately | The user-reviewed decision surface may vary by transport without owning pending state |
| Executor | The live runtime sends only admitted, allowed calls to filesystem and process adapters, which enforce their own hard constraints | Network and MCP adapters enter through the same boundary |

## Failure modes

| Situation | Response |
| --- | --- |
| Unknown tool or malformed arguments | Typed admission refusal; no approval request and no execution |
| Policy forbids a valid call | Typed forbidden result; approval cannot be requested to bypass it |
| Approval transport is absent | The call remains pending; never a fail-open default |
| A decision arrives after denial, cancellation or completion | Typed stale decision; no slot changes |
| The turn is interrupted or the runtime shuts down while waiting | The slot is cancelled and its result debt is paid before the turn closes |
| Allowed siblings finish while another waits | Their results remain in their slots and reach the model only when the ordered batch is complete |
| A persisted pending request is restored | It is cancelled as `process_died`; it cannot continue. A later explicit submission may produce a new call, which uses current definitions and rules |

## Evidence

[Named proofs](../evidence/tool-admission.md), one row an invariant.
