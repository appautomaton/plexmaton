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
the way back — it takes no rows at all. Its top rule meets the activity row without an extra
bottom border or blank row. It is chrome, so a row past its rectangle is a row with no
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

| Invariant | Proven by |
| --- | --- |
| IQU-1 | `a_message_typed_mid_turn_is_reported_until_its_boundary_claims_it`, `completed_skill_input_wakes_the_waiting_projection_without_a_model_delta`, `input_held_by_an_owned_operation_is_reported_and_taken_back_newest_first`, `every_boundary_names_itself_once_above_its_own_entries`, `waiting_input_is_reported_above_the_composer_at_every_width` with the `input-queue-*` frames |
| IQU-2 | `the_waiting_band_yields_to_both_inputs_and_to_a_readable_conversation`, `height_is_bounded_by_what_is_listed_rather_than_by_what_is_waiting`, `a_multiline_message_occupies_one_row_without_joining_its_lines`, `a_sending_time_the_band_stopped_listing_is_counted_rather_than_named`, `a_band_cut_short_drops_messages_before_it_drops_the_way_back`, `a_registered_band_shows_every_waiting_message_and_the_way_back`, `waiting_input_is_reported_above_the_composer_at_every_width` |
| IQU-3 | `chrome_is_neither_a_pointer_target_nor_a_focus_stop`, `every_registered_surface_is_drawn_inside_its_own_bounds`, `alt_up_takes_back_the_last_waiting_message_and_a_bare_arrow_still_moves_the_caret`, `the_way_back_is_inert_until_something_waits_and_then_names_the_primary` |
| IQU-4 | `the_newest_waiting_message_comes_back_with_its_exact_text`, `input_held_by_an_owned_operation_is_reported_and_taken_back_newest_first`, `taking_a_message_back_leaves_the_running_turn_alone`, `waiting_input_keeps_an_existing_draft_and_its_skill_separate`, `the_way_back_is_inert_until_something_waits_and_then_names_the_primary`, `a_live_dispatch_restores_undelivered_user_text` for the composer it lands in |

Reviewed frames: [wide](../../crates/plexmaton-tui/frames/input-queue-wide.txt),
[medium](../../crates/plexmaton-tui/frames/input-queue-medium.txt),
[narrow](../../crates/plexmaton-tui/frames/input-queue-narrow.txt).

The [executable journey](../../scripts/smoke-input-queue.py) proves IQU-1/IQU-4 with a paused
loopback stream and real `Alt-↑` bytes: occupied-draft refusal, exact multiline text and numeric
skill binding, unchanged journal bytes during withdrawal, continued first response, and the
remaining queued message before explicit resubmission. It captures actual single-agent frames
at 120/88/60 columns. The fixture uses three local requests and no live credentials or model.

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
