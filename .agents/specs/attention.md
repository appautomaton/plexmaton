# Spec — Attention queue

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | What happens when a background agent needs the user, and what the user can do about it |
| Depends on | The attention rules in [`ui-ux.md`](../ui-ux.md) §attention management; the focused-surface grammar in [interaction-routing](./interaction-routing.md) INV-10 |
| Proven by | `plexmaton-tui::state::attention` and `::workspace` tests |

## Invariants

**ATT-1 — Arriving costs the user nothing.** A queued request moves no keyboard focus, changes no
selected agent, opens no dismissible surface, moves no text cursor, and disturbs no draft. The queue
is chrome like the notice strip (ui-ux §attention management): it appears under the strip when it
has something to say, and its rows come out of what has already been read, so the newest
conversation rows and the composer stay where they are.

**ATT-2 — Going to a request is a keypress the user made.** The queue has its own cursor, and
`Enter` on it selects the requesting agent and moves the keyboard there. No producer path reaches
that verb: the intent enum it lives in has no producer caller.

**ATT-3 — Acknowledging is not resolving.** Going to a request marks it seen and leaves it queued,
because it is still outstanding. Only `AttentionResolved` from the owning loop clears it; closing
an approval surface or sending a decision does not optimistically remove loop-owned state. An
agent that asks again arrives unseen.

## Model

```text
SessionEvent::AttentionRequested ──▶ AttentionQueue (arrival order, coalesced by AttentionId)
                                            │
   user presses Enter on the queue ─────────┴──▶ acknowledge + select the requesting agent
                                            │
SessionEvent::AttentionResolved ─────────────┴──▶ remove the exact request
```

| Fact | Value |
| --- | --- |
| Coalescing | By `AttentionId`: a repeat replaces its entry in place and keeps its position, so an agent asking twice is one item |
| Place | A `Panel` under the notice strip, registered while the queue is non-empty: a focus stop, a pointer target, scrollable |
| Height | Three requests, then it scrolls rather than grows. On a short terminal it yields its rows before the notice strip does, because a blocked agent also reads as `Waiting` in the rail |
| Counts | The rail's title wears a badge, `!n`, in the action-required role only while `n` requests are unanswered, and nothing otherwise; the word is the band's, whose title carries both numbers |

## Failure modes

| Situation | Response |
| --- | --- |
| A request from an agent not in the roster | Rejected at ingest as an unknown agent, which reaches the notice log |
| `Enter` on an empty queue | A no-op that does not advance the revision |
| Going to a request whose agent has since left the roster | The acknowledgement stands and the selection does not move |
| A resolution names another agent's request | Rejected as an ownership mismatch and shown in the notice log |
| More requests than the band lists | The band keeps its height and the rest arrive by scrolling it |
| A click on a row | Focuses the band; the cursor moves by keyboard |
| A terminal too short for the band and a comfortable conversation | The band is not registered; the rail's count remains |

## Evidence

| Invariant | Proven by |
| --- | --- |
| ATT-1 | `a_background_request_takes_no_focus_no_selection_and_no_cursor`, `the_journey_keeps_a_second_agent_on_screen_and_takes_a_request_without_being_interrupted` |
| ATT-2 | `going_to_a_request_is_the_users_move_and_marks_it_seen`, `the_queues_cursor_moves_without_touching_the_agent_selection`, `the_cursor_clamps_at_both_ends_and_survives_an_empty_queue` |
| ATT-3 | `acknowledging_marks_one_request_and_a_repeat_unmarks_it`, `an_agent_asking_twice_produces_one_queue_item`, `resolving_removes_only_the_named_request_and_repairs_the_cursor`, `an_open_approval_blocks_the_workspace_and_returns_only_the_selected_decision` |
