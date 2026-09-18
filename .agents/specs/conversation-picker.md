# Spec — Conversation picker

| Field | Value |
| --- | --- |
| Status | Implemented; evidence below |
| Owns | `/new` and `/resume`: bounded discovery of saved conversations, the user's choice among them as composer menu rows, live conversation replacement, and the collaboration the replacement carries |
| Depends on | INV-1, INV-6, JRN-4, JRN-5, CTL-1, CHB-3, SCH-4; [composer-menu](./composer-menu.md) CMC-1/CMC-2 and SKP-3/SKP-4 for the rows' geometry and routing |
| Proven by | CLI `session_picker` and TUI `workspace::composer_menu_tests` |

## Invariants

**SPK-1 — The saved conversations are `/resume`'s rows.** `/resume` lists saved history in the
composer menu, newest first, filtered by the text after it; `/new` is a Command row (CMC-1).
Keyboard and matching mouse press/release choose one row; wheels move the choice without focus
changes, and dragging cancels activation. A row leaves the workspace as a `ConversationRequest`,
new or by identity. The listing's one status row says what stands in for rows: loading, no match,
a failed listing, an open in flight, or why the chosen row did not open; the rows can be chosen
again after a refusal. A partial listing is labelled in the menu's title. Short terminals keep the
selected row and controls visible. Search reads bounded display previews, never provider replay
payloads. Listing examines at most 10,000 directory entries, retains at most 200 newest candidates
by modification time and reads at most 64 KiB per preview. Files are not repaired, created or
deleted during listing.

**SPK-2 — Validate before replacing.** A switch needs an idle runtime and no unsent input,
including a displaced edit/retry draft; `/resume` and its query are the request, not a draft. A
delegated child still working does not block it: the first choice says what the switch would cost
that child, and the same choice again performs it and stops the child through SCH-4's Stop. Only
the same request with the same children still working consumes an offer — a different row, a new
draft, the root starting work, or the child finishing on its own all put the question back. The
children a confirmed switch stops travel with its request, so nothing is asked a second time when
the candidate lands. `/new` has no row to choose again, so its second gesture is the command
itself. Target loading uses JRN-4/JRN-5 before replacing the current runtime and projection.
Locked, corrupt or missing targets leave the current conversation and draft intact. A refused or failed switch
is answered where the request still is: the listing's status row for a chosen row, and one sentence
after the conversation's last entry for `/new`, whose draft was consumed on acceptance. An offered
one is not — it is the workspace waiting for a gesture, so it arms the terminal's last row beside
the quit chord (INV-7), leaving the rows the user is choosing between whole. A listing's own status
row wraps rather than truncating: the half a narrow terminal would cut is the half that says what
to do. Selecting the current conversation consumes the `/resume` draft and nothing else.
Restoration never submits input or dispatches model/tool work. A new conversation uses lazy
automatic storage (JRN-4), or stays ephemeral when replacing an ephemeral conversation; previous
files remain untouched. The exit handoff prints `To continue this session, run:` and
`plexmaton resume <id>` for only the selected saved conversation. An unsaved blank conversation or
ephemeral session prints no handoff.

**SPK-3 — The loader has an owner.** The CLI owns at most one listing/loading job, and knows
which it is. File operations run in the blocking pool; results arrive through the terminal loop's
select. `Escape`, or a draft that stops asking for the rows while nothing is opening, withdraws
permission: a completed candidate is shut down and its writer released, and a listing asked for
again before the withdrawn one landed is listed afresh. Quit cancels discovery and joins any pending
job before shutting down the active runtime. A second request while one runs is refused, never a
second loader. Cancellation during an existing file open finishes that open before cleanup; it
does not detach a worker or promise to interrupt a filesystem syscall.

Rejected: reading JSONL from widgets; replacing the current runtime before knowing the target is
usable; silently dropping a draft, or interrupting active work to change conversations without
first saying what it costs and being told again; a flat refusal that leaves the user to find the
child and stop it themselves, which makes the product an obstacle to a decision it has already been
told; global Retry commands, because they are operations on an eligible failed message rather than
discovery; a Drawer page for conversations, which hid what users type by habit behind a chord; and
a confirmation of its own inside the listing, which invented a second grammar for a question the
quit chord already had one for, and spent two of the rows the user was choosing between to ask it.

**SPK-4 — The replacement carries its collaboration.** A conversation that can delegate gets its
own collaboration log, Main tool lane and child factory when the picker opens it, bound to the exact
runtime instance that will hold it (CTL-1) and restored before the projection is replaced (CHB-3).
That work happens on the loader, not on the frame that accepts the switch. The conversation being
replaced is joined exactly once, before its runtime, and a candidate nobody takes releases its log
as well as its runtime — a log still locked cannot be opened again. An ephemeral replacement has no
durable identity and therefore no collaboration, and `delegate` is absent from its tools.

Rejected: rebinding the outgoing collaboration to the incoming runtime, which its own
conversation-and-instance check exists to refuse; and installing the lane after the runtime is
built, which is not possible — Main authorship does not exist on a runtime whose catalog never
carried the lane.

## Rendered review

The offered switch at the three product widths, exported from real buffers by
`cargo run -p plexmaton-tui --example switch_confirmation_preview -- crates/plexmaton-tui/frames/switch-confirmation`.

| Where | Wide | Medium | Narrow |
| --- | --- | --- | --- |
| `/resume`, under its rows | [120](../../crates/plexmaton-tui/frames/switch-confirmation/switch-confirm-120.svg) | [88](../../crates/plexmaton-tui/frames/switch-confirmation/switch-confirm-88.svg) | [60](../../crates/plexmaton-tui/frames/switch-confirmation/switch-confirm-60.svg) |
| `/new`, after the last entry | [120](../../crates/plexmaton-tui/frames/switch-confirmation/switch-new-120.svg) | [88](../../crates/plexmaton-tui/frames/switch-confirmation/switch-new-88.svg) | [60](../../crates/plexmaton-tui/frames/switch-confirmation/switch-new-60.svg) |

At 60 the sentence wraps onto two rows and the listing keeps every row it had; the `/new` note keeps
the draft that asked for it, so the second gesture is one key.

## Evidence

| Invariant | Proven by |
| --- | --- |
| SPK-1 | `a_wrapped_status_says_its_whole_sentence_and_reserves_the_rows_it_draws`, `resume_lists_saved_conversations_by_identity_for_keyboard_and_pointer` with the `composer-menu-resume-loading-medium` and `composer-menu-resume-medium` frames, `resume_status_rows_cover_failure_no_match_and_opening`, `listing_is_bounded_read_only_and_rejects_symlinks` |
| SPK-2 | `a_switch_that_stops_a_working_child_asks_on_the_row_the_quit_chord_uses`, `a_switch_past_a_working_child_is_offered_once_and_only_the_same_choice_takes_it`, `only_a_child_with_work_to_lose_is_offered_for_a_switch`, `session_switch_validates_before_replacing_and_never_dispatches`, `cancelled_picker_releases_candidate_and_preserves_current_draft`, `new_session_is_lazy_and_replacement_preserves_saved_history`, `new_session_refuses_unsent_input_and_active_work`, `continuation_handoff_names_only_the_selected_saved_session`, `a_refused_switch_is_a_note_and_the_listing_offers_its_rows_again` |
| SPK-4 | `switching_carries_a_live_collaboration_into_the_replacement`, `an_ephemeral_replacement_has_no_collaboration_and_no_delegate_tool` |
| SPK-3 | `cancelled_picker_releases_candidate_and_preserves_current_draft`, `new_session_is_lazy_and_replacement_preserves_saved_history`, `a_refused_switch_is_a_note_and_the_listing_offers_its_rows_again`. Listing afresh after a withdrawal is unproven |
