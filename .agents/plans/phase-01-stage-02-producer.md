# Plan — Phase 01 stage 2, the producer

| Field | Value |
| --- | --- |
| Phase | [Phase 01 — One real agent](../phases/phase-01-one-real-agent.md) §scope 2 |
| Contract | [UI/UX](../ui-ux.md) §product vocabulary, §progressive disclosure, §state matrix |
| Status | Slices 1 and 2 of 7 landed 2026-09-02; slice 3 next |
| Blocked | Slice 5 only, on the provider choice, which is the user's |

## Outcome

A real model streams into the primary conversation and runs tools there, behind a semantic
boundary that did not move. The projection cannot tell that the producer changed from a scripted
timeline to a model: it still receives `SessionEvent` on one monotonic sequence, still refuses a
gap, and the checked-in frames still match.

## The loop's contract, until code cites it

Kept inline here, and promoted to `specs/agent-loop.md` the moment an identifier below appears in
code — a citation must outlive the plan.

**LOOP-1 — A turn is one or more steps, and the budget is counted in steps.** A step is one
request to the model and the tool calls it returns; the turn ends when a step's stop reason is not
a tool call. Exhausting the budget is a typed failure with a visible warning, never a silent stop.

**LOOP-2 — Every dispatched tool call owes a result.** A call the model made and the loop
dispatched is answered, including when the turn is cancelled: a call interrupted before it ran is
answered as aborted rather than dropped. A tool call with no result makes the *next* request
malformed, so the defect surfaces a turn later than the mistake.

**LOOP-3 — Results are model-ordered.** Calls whose declared effects do not conflict may overlap,
bounded; the batch is assembled in the order the model emitted it, whatever order it finished in.
Which calls may overlap is decided by a pure function over declared effects.

**LOOP-4 — The turn is one value.** Interrupt at any point and the state says where it stopped and
what it owes. Nothing that a frame must show or a future session must resume lives in a stack
frame, a callback, or an adapter's privates — provider replay metadata included.

**LOOP-5 — Approval is state, not a suspended call.** A call whose effect is not read-only parks
the turn in a pending record the Attention queue projects; the agent reads waiting and the
composer stays live. The decision arrives as a command and its resolution leaves as an event.

Rejected, from the four implementations read on 2026-09-02: a turn as one `async fn`, whose state
is its stack frame and whose only test is a mocked socket (`codex-rs` `run_turn`); an approval as
`Arc<dyn Fn(..) -> BoxFuture<Decision>>`, which cannot be counted, rendered, or resumed
(`pi_agent_rust`); cancellation as an error variant, which makes every call site decide separately
what an abort meant; and one dialect's wire vocabulary as the shared event type, which leaves the
second adapter fabricating fields (`codex-api::ResponseEvent`).

## Slices

1. **The turn machine.** Landed. `plexmaton-agent`: the session record, the turn state, and a
   `handle` that takes a typed input and returns events plus effects. One turn is one step, because
   no tools exist yet to ask for a second. Closed by a stream the projection accepted with an empty
   notice log, and by a gate that refuses a runtime, a client or a terminal anywhere in this
   crate's dependency closure.
2. **Steps, tool calls, and the debt.** Landed. A turn is several steps under a budget counted in
   them (LOOP-1), a step dispatches its calls as one batch and is answered in model order
   (LOOP-3), and an interrupt or a failure pays what the batch owes before going idle. Closed by
   the debt asserted at every point an interrupt can land, shown to fail when the payment is
   removed.
3. **Input routing.** The next-step queue and the boundary claim, slice 1 having landed the
   next-turn half; `Ctrl-C` interrupts a running turn instead of only clearing the draft. *Closes when* text
   submitted while a step is in flight lands in the turn the user meant, and an undeliverable one
   keeps its text.
4. **Approval.** A pure policy over declared effects, a pending record on the turn, the command
   that answers it and the event that empties the queue. *Closes when* the badge and the queue
   count come from state alone, and answering resumes that exact call (LOOP-5).
5. **The provider adapter.** `plexmaton-provider`: SSE to a semantic `ModelEvent`, a typed
   `StopReason`, a typed error taxonomy, and tool arguments accumulated across deltas and parsed
   once. *Closes when* a recorded fixture drives a full turn under `cargo test` with no network.
6. **Tools.** Read, search, run a command: declared effects, workspace-rooted paths with a typed
   refusal for an escape, output bounded with a visible truncation marker. *Closes when* a
   megabyte of stdout neither grows the transcript without bound nor the next request.
7. **Live.** The executable pushes producer events instead of polling a tick; the simulator stays
   the test producer. *Closes when* the user talks to a real model, with frames at wide, medium
   and narrow looked at.

## Order, and why

1 first because every later slice is expressed in its states. 2 before 3, because what an
interrupt owes constrains what a queue may do at a boundary. 4 before 5, because approval is state
the adapter must never own, and writing it second invites the adapter to keep it. 5 before 6,
because what a tool result must look like on the wire shapes the tool surface, not the reverse. 7
last: it is the slice that cannot be tested without a key.

Slices 1 to 4 need no provider, so the choice blocks nothing until 5.

## Deliberately not in this plan

Persistence and replay, context projection and compaction, a second provider, MCP: Phase 02. A
second agent, delegation, mail, and the mailbox: Phase 03. The transcript grammar for tool calls,
diffs and reasoning, and retiring the Activity panel: stage 3 of this phase. Streaming a tool's
partial output while it runs, which stage 3 will want and this stage must not design blind.
