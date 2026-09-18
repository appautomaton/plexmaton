# Spec — Agent loop

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Turn and step lifecycle, input-boundary routing, tool-call debt and approval state |
| Depends on | [UI/UX](../ui-ux.md) §state matrix for queued and undelivered steering |
| Proven by | `plexmaton-agent::turn` tests and the executable's boundary tests |

## Invariants

**LOOP-1 — A turn is one or more budgeted steps.** A step is one model request and the tool calls
it returns. Exhausting the step budget is a typed, visible failure rather than a silent stop.

**LOOP-2 — Every dispatched tool call owes a result.** A call the loop dispatched is answered even
when the turn fails or is interrupted; an interrupted call is answered as aborted.

**LOOP-3 — Results are model-ordered.** Calls whose effects permit it may finish concurrently, but
the batch shown to the model is assembled in the order the model emitted it.

**LOOP-4 — A turn is one value.** Its lifecycle state, provider replay metadata and outstanding
debt are inspectable state rather than a stack frame, callback or adapter private.

Rejected: a turn as one `async fn`, and cancellation as an error propagated for each caller to
interpret; neither leaves one state that can be rendered, resumed and tested between inputs.

**LOOP-5 — Approval is state, not a suspended call.** A call requiring approval parks the turn in
a pending record; a typed command resolves that record and resumes that exact call.

Rejected: an approval callback returning a future, which cannot itself be counted, rendered or
resumed.

**LOOP-6 — User input names the boundary it means.** A submitted message waits for the next turn;
steering waits for the current turn's next step. The loop claims it only while opening that
boundary, and returns an unclaimable input with its exact text and a typed reason.

Rejected: one queue drained at whichever boundary happens first, which silently turns steering
into a later conversation or a new message into part of a turn already in flight.

## Model

```text
Submitted ──▶ next-turn queue ──▶ turn boundary ──▶ user item + step 1
Steered   ──▶ next-step queue ──▶ step boundary ──▶ user item + next step
                     │
                     └─ no matching boundary ──▶ UndeliveredInput(text, reason)
```

## Evidence

[Named proofs](../evidence/agent-loop.md), one row an invariant.
