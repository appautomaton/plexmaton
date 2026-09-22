# Spec — Waiting input

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What the workspace shows about input the user submitted that no request carries yet, and how the user takes one back |
| Depends on | LOOP-6 for the boundaries, CPL-9 for the admission door, SURF-3 for what chrome is, INV-2 and COM-1 for the composer this band sits on |
| Proven by | Runtime queue tests, TUI layout/state/frame tests and the offline executable journey below |

## Invariants

**IQU-1 — The agent owns the queue; the workspace only shows it.** The workspace reads the whole
queue again each frame and replaces its bounded display snapshot; it owns no independent queue
to reconcile when a message is sent. A completed skill submission wakes the composition root
through a data-free dispatch report, so a silent provider cannot leave the snapshot stale.
Each entry carries the exact text the user
submitted and where it is waiting: for the current turn's next step, for the next turn, or in the
runtime, which has not handed it to the agent yet because an owned operation is running
(LOOP-6, CPL-9). None of it is in the session journal — no entry, no sequence number, no
acknowledgement — so it is not a transcript entry and cannot be anchored, selected or copied as one.
Rejected: a durable event when a message is queued and released, which would give a journal entry
and a sequence number to something that may never be sent; and drawing the waiting message as a
transcript row, which would put it at a position `Ctrl-C` then takes away.

**IQU-2 — The band is a section of its conversation, and it yields.** It sits between the
conversation and the decision region, in the same box as the composer, because the user's own
`Enter` put it there. It bids for rows after both inputs and takes none from either; its rows come
out of the conversation, and it takes none at all rather than leave a conversation too short to
read. Its height follows what it lists, not what is waiting or how much was typed: one row per
entry, at most three listed, one heading per place a message it lists is waiting, and a count of
every entry it did not list.

Cut below what it asked for, it lists fewer entries rather than losing the rows off its bottom, and
the count grows to cover them; below its floor — its rule, one heading, one entry, that count and
the way back — it takes no rows at all. Its top rule follows the blank row the activity line keeps
beneath itself, with no bottom border or blank row of its own. It is chrome, so a row past its rectangle is a row with no
way to reach it: what it cannot show it must count, and the way back is the last row it gives up.
Rejected: letting the rows it was granted simply clip its content, which drops the key in IQU-3
first and leaves the title counting messages the band has stopped showing.

**IQU-3 — The way back is a key on the composer, not a surface.** The band stays chrome: never a
focus stop, never a pointer target (SURF-3). `Alt-↑`, pressed with the cursor in the empty primary
composer, acts on it; the band prints that key beside the messages it applies to. A cursor in a
worker's window addresses that worker, so it withdraws nothing here (COM-4).
Rejected: making the band a focus stop with per-row actions. That buys reordering and choosing
which entry to act on, at the price of a second menu grammar, for a queue that is nearly always one
or two messages deep.

**IQU-4 — Withdrawing returns one exact message into an empty draft.** `Alt-↑` takes the
message the user submitted most recently — wherever it was waiting, because the user is undoing one
`Enter` rather than picking a queue — and gives it back with its text and any explicit skill, through
the same `undelivered` path that already returns input no boundary could claim. It appends nothing
to the session journal, sends no model request, runs no tool, opens no turn or step, and does not
disturb the turn that is running. The band re-reads the queue each frame, so the entry simply is
not there next frame. An occupied primary draft blocks the request before runtime mutation;
the band says `Alt-↑ needs an empty draft`. Both messages and their skill bindings remain owned
where they were. With nothing waiting the key does nothing.
Rejected: merging returned input into an existing draft, which conflates messages and loses skill
identity; and using `Ctrl-C` for single-message retrieval, which interrupts the turn and drains all
waiting input under LOOP-6.

## Evidence

[Named proofs](../evidence/input-queue.md), one row an invariant.

## Model

Steering the current turn from the primary composer: today every submission from it is a message
for the next turn, so `queued_for_next_step` is reachable only from a worker's own input. Whether
mid-turn `Enter` should steer instead is a contract question, not an implementation gap.

Reordering the queue, and sending a waiting message early. Both need the band to become a surface
the user can point at a row of, which IQU-3 declines until a queue that deep is real.

The one message a skill file read is running for. It waits apart from the queue the runtime reports,
so between its `Enter` and the read settling it is not in the band and `Alt-↑` does not reach it;
messages submitted behind it do appear, as waiting on that operation. Showing it without being able
to take it back would break IQU-4, and taking it back means cancelling a read this mechanism does
not own.
