# Spec — Attention queue

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What happens when a background agent needs the user, and what the user can do about it |
| Depends on | The attention rules in [`ui-ux.md`](../ui-ux.md) §attention management; the focused-surface grammar in [interaction-routing](./interaction-routing.md) INV-10 |
| Proven by | `plexmaton-tui::state::attention` and `::workspace` tests; see the evidence table |

## Purpose

A delegating workspace has agents that get stuck. The whole point of delegating is that the user is
somewhere else when it happens, so the mechanism that tells them has to be one that does not take
them back.

This is also the reason the workspace has no modal surface at all. The one thing that would have opened
one is a background approval, and this exists so it does not.

## Invariants

**ATT-1 — Arriving costs the user nothing.** A queued request does not move keyboard focus, change
the selected agent, open a dismissible surface, move the text cursor, or disturb a draft. Being
visible is the whole of what it does.

**ATT-2 — Going to a request is a keypress the user made.** The queue has its own cursor, and
`Enter` on it selects the requesting agent and moves the keyboard there. Nothing on the producer
path can reach that verb — the intent enum it lives in has no producer caller — so "the user chose
to" is structural rather than a rule to be remembered.

**ATT-3 — Acknowledging is not resolving.** Going to a request marks it seen, and a seen request
stays queued, because it is still outstanding. What clears one is the agent being unblocked, which
needs an approval the runtime cannot yet grant. An agent that asks again arrives unseen.

## Model

```text
PrototypeEvent::AttentionRequested ──▶ AttentionQueue (arrival order, coalesced by identity)
                                            │
   user presses Enter on the queue ─────────┴──▶ acknowledge + select the requesting agent
```

Coalescing is by `AttentionId`: a repeated request replaces its entry in place and keeps its
position, so an agent that asks twice produces one item rather than a notification storm.

### Where it is, and why that is not "opening a surface"

The band is registered whenever the queue is non-empty and drawn under the notice strip at the top
of the screen, so its rows come out of what has already been read: the newest conversation rows and
the composer stay where they are, and a request arriving never moves the cursor (ATT-1). It is a `Panel`: a
focus stop, a pointer target, and scrollable, because the queue is unbounded and the band lists
three requests before it starts scrolling instead of growing.

`ui-ux.md` says background agents never "open a surface" and also that action-required items enter a
"visible, ordered Attention queue". Those meet here: the band is workspace chrome that appears when
it has something to say, exactly as the notice strip does, and neither has ever been what "opening a
surface" meant. What is forbidden is a layer over the user's work that takes focus or blocks input,
and the band is none of those.

Rows come after the notice strip in priority, so on a short terminal the band is what yields. The
reason is detectability, the same one that puts the strip above the agent list: a silently wrong
projection has no other signal, while a blocked agent also reads as `Waiting` in the rail and its
request returns the moment the rows do.

### Counts

The rail counts **unanswered** requests, not queued ones. A queue of five the user has already been
to is not five things demanding them, and a count that kept shouting after they did what was asked
would train them to ignore it. The band's own title carries both numbers, because it is the surface
with room to show the difference.

## Failure modes

| Situation | Response |
| --- | --- |
| A request from an agent not in the roster | Rejected at ingest as an unknown agent, which is a producer defect and reaches the notice log |
| `Enter` on an empty queue | A no-op that does not advance the revision |
| Going to a request whose agent has since left the roster | The acknowledgement stands and the selection does not move: the user did see it |
| More requests than the band lists | The band keeps its height and the rest arrive by scrolling it |
| A terminal too short for both the band and a comfortable conversation | The band is not registered at all; the rail's count is what remains |

## Out of scope

- **Resolving a request.** There is no `AttentionResolved` event, because no producer can yet emit one
  honestly. It arrives with real tools and real approvals (Phase 01).
- **The other direction of the relationship.** `ui-ux.md` asks the queue to carry a delegating agent
  objecting to something the user changed. Same mechanism, no producer yet.
- **Acting on a request from inside the queue.** Approving or answering in place needs a reply
  channel the runtime does not have. Going to the agent is what the workspace offers today.
- **Clicking a row to act on it.** A press focuses the band; the cursor moves by keyboard. The
  contract requires every mouse gesture to have a keyboard equivalent, not the reverse.

## Evidence

| Invariant | Proven by |
| --- | --- |
| ATT-1 | `a_background_request_takes_no_focus_no_selection_and_no_cursor`, `the_journey_keeps_a_second_agent_on_screen_and_takes_a_request_without_being_interrupted` |
| ATT-2 | `going_to_a_request_is_the_users_move_and_marks_it_seen`, `the_queues_cursor_moves_without_touching_the_agent_selection`, `the_cursor_clamps_at_both_ends_and_survives_an_empty_queue` |
| ATT-3 | `acknowledging_marks_one_request_and_a_repeat_unmarks_it`, `an_agent_asking_twice_produces_one_queue_item`, `the_journey_copies_evidence_and_returns_to_the_prior_state` |
