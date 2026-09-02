# Plan — Phase 01 stage 2, the producer

| Field | Value |
| --- | --- |
| Phase | [Phase 01 — One real agent](../phases/phase-01-one-real-agent.md) §scope 2 |
| Contract | [UI/UX](../ui-ux.md) §product vocabulary, §progressive disclosure, §state matrix |
| Status | Slices 1–3 of 7 landed 2026-09-02; slice 4 next |
| Blocked | Slice 5 only, on the provider choice, which is the user's |

## Outcome

A real model streams into the primary conversation and runs tools there, behind a semantic
boundary that did not move. The projection cannot tell that the producer changed from a scripted
timeline to a model: it still receives `SessionEvent` on one monotonic sequence, still refuses a
gap, and the checked-in frames still match.

## The loop's contract

[`agent-loop.md`](../specs/agent-loop.md) owns LOOP-1 through LOOP-6 now that their identifiers are
cited by the implementation and tests. This plan owns only the order in which the mechanism lands.

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
3. **Input routing.** Landed. Submitted messages queue for the next turn, steering queues for the
   current turn's next step, and each bounded route is claimed only while its boundary opens
   (LOOP-6). An input with no boundary returns with its exact text and a typed reason. The
   composition root maps visible routes and `Ctrl-C` to `Input`; its synthetic adapter reports that
   it cannot perform an interrupt, and slice 7 replaces that adapter with the live loop owner.
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
