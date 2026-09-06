# Spec — Conversation picker

| Field | Value |
| --- | --- |
| Status | Implemented; evidence below |
| Owns | `/new` and `/resume`: bounded discovery of saved conversations, the user's choice among them as composer menu rows, and live conversation replacement |
| Depends on | INV-1, INV-6, JRN-4, JRN-5; [composer-menu](./composer-menu.md) CMD-1/CMD-2 and SKP-3/SKP-4 for the rows' geometry and routing |
| Proven by | CLI `session_picker` and TUI `workspace::composer_menu_tests` |

## Invariants

**SPK-1 — The saved conversations are `/resume`'s rows.** `/resume` lists saved history in the
composer menu, newest first, filtered by the text after it; `/new` is a Command row (CMD-1).
Keyboard and matching mouse press/release choose one row; wheels move the choice without focus
changes, and dragging cancels activation. A row leaves the workspace as a `ConversationRequest`,
new or by identity. The listing's one status row says what stands in for rows: loading, no match,
a failed listing, an open in flight, or why the chosen row did not open; the rows can be chosen
again after a refusal. A partial listing is labelled in the menu's title. Short terminals keep the
selected row and controls visible. Search reads bounded display previews, never provider replay
payloads. Listing examines at most 10,000 directory entries, retains at most 200 newest candidates
by modification time and reads at most 64 KiB per preview. Files are not repaired, created or
deleted during listing.

**SPK-2 — Validate before replacing.** Only an idle runtime with no unsent input, including a
displaced edit/retry draft, can switch conversations; `/resume` and its query are the request,
not a draft. Target loading uses JRN-4/JRN-5 before replacing the current runtime and projection.
Locked, corrupt or missing targets leave the current conversation and draft intact. A refused or
failed switch is answered where the request still is: the listing's status row for a chosen row,
and one sentence after the conversation's last entry for `/new`, whose draft was consumed on
acceptance. Selecting the current conversation consumes the `/resume` draft and nothing else.
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
usable; silently dropping a draft or interrupting active work to change conversations; global Retry
commands, because they are operations on an eligible failed message rather than discovery; a
Drawer page for conversations, which hid what users type by habit behind a chord.

## Evidence

| Invariant | Proven by |
| --- | --- |
| SPK-1 | `resume_lists_saved_conversations_by_identity_for_keyboard_and_pointer` with the `composer-menu-resume-loading-medium` and `composer-menu-resume-medium` frames, `resume_status_rows_cover_failure_no_match_and_opening`, `listing_is_bounded_read_only_and_rejects_symlinks` |
| SPK-2 | `session_switch_validates_before_replacing_and_never_dispatches`, `cancelled_picker_releases_candidate_and_preserves_current_draft`, `new_session_is_lazy_and_replacement_preserves_saved_history`, `new_session_refuses_unsent_input_and_active_work`, `continuation_handoff_names_only_the_selected_saved_session`, `a_refused_switch_is_a_note_and_the_listing_offers_its_rows_again` |
| SPK-3 | `cancelled_picker_releases_candidate_and_preserves_current_draft`, `new_session_is_lazy_and_replacement_preserves_saved_history`, `a_refused_switch_is_a_note_and_the_listing_offers_its_rows_again`. Listing afresh after a withdrawal is unproven |
