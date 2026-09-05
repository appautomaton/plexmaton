# Spec — Session picker

| Field | Value |
| --- | --- |
| Status | Implemented; evidence below |
| Owns | Bounded session discovery, user selection and live conversation replacement |
| Depends on | INV-1, INV-6, INV-11, JRN-4, JRN-5 |
| Proven by | CLI `session_picker` and TUI `workspace::session_picker_tests` |

## Invariants

**SPK-1 — One searchable history entry point.** `/resume` and aliases `/continue`, `/sessions`,
`/session` open the same Sessions mode of the command palette. Keyboard and matching mouse
press/release choose one typed session identity; wheels move the choice without focus changes,
and dragging cancels activation. Short windows keep the selected row and controls visible. Search reads bounded
display previews, never provider replay payloads. Listing examines at most 10,000 directory entries,
retains at most 200 newest candidates by modification time and reads at most 64 KiB per preview;
a partial listing is labelled. Files are not repaired, created or deleted during listing.
`/new` opens an empty conversation through the same replacement owner, without listing history
or submitting model input.

**SPK-2 — Validate before replacing.** Only an idle runtime with no unsent input, including a
displaced edit/retry draft, can switch sessions. Target loading uses JRN-4/JRN-5 before replacing
the current runtime and projection. Locked, corrupt or missing targets leave the current session
and draft intact and show a local picker error. Selecting the current session closes the picker.
Restoration never submits input or dispatches model/tool work. A new conversation uses lazy automatic
storage (JRN-4), or stays ephemeral when replacing an ephemeral conversation; previous files remain
untouched. The exit handoff prints `To continue this session, run:` and `plexmaton resume <id>` for
only the selected saved session. An unsaved blank conversation or ephemeral session prints no handoff.

**SPK-3 — The loader has an owner.** The CLI owns at most one listing/loading job. File operations
run in the blocking pool; results arrive through the terminal loop's select. Closing the picker
withdraws permission to switch; a completed candidate is shut down and its writer released. Quit
cancels discovery and joins any pending job before shutting down the active runtime. Repeated
activation cannot start a second loader. Cancellation during an existing file open finishes that
open before cleanup; it does not detach a worker or promise to interrupt a filesystem syscall.

Rejected: reading JSONL from widgets; replacing the current runtime before knowing the target is
usable; silently dropping a draft or interrupting active work to change sessions; global Retry
commands, because they are operations on an eligible failed message rather than session discovery.

## Evidence

| Invariant | Proven by |
| --- | --- |
| SPK-1 | `resume_aliases_share_one_command_and_retry_is_not_a_global_command`, `new_command_keyboard_and_pointer_emit_the_same_intent`, `listing_is_bounded_read_only_and_rejects_symlinks`, `session_picker_keyboard_and_mouse_share_identity_and_cancel_drags`, `session_picker_frames_cover_empty_populated_and_failure_states`, `short_session_picker_keeps_selected_result_and_footer_visible`, `short_command_palette_and_wheel_use_the_visible_choice_window` |
| SPK-2 | `session_switch_validates_before_replacing_and_never_dispatches`, `cancelled_picker_releases_candidate_and_preserves_current_draft`, `new_session_is_lazy_and_replacement_preserves_saved_history`, `new_session_refuses_unsent_input_and_active_work`, `continuation_handoff_names_only_the_selected_saved_session` |
| SPK-3 | `cancelled_picker_releases_candidate_and_preserves_current_draft`, `new_session_is_lazy_and_replacement_preserves_saved_history` |
