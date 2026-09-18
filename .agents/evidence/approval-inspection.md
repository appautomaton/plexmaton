# Evidence — Approval command inspection

What proves [approval-inspection](../specs/approval-inspection.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| APD-1 | `cmd_1_command_detail_preserves_multiline_source_and_context`, `command_modal_copies_exact_source_and_returns_to_the_pending_approval`, `command_controls_are_visible_text_not_terminal_sequences` |
| APD-2 | `command_modal_copies_exact_source_and_returns_to_the_pending_approval`, `drawer_keeps_focus_when_the_inspected_approval_resolves`, `command_summary_press_pins_the_complete_request_identity` |
| APD-3 | `command_modal_copies_exact_source_and_returns_to_the_pending_approval`; `command_modal_copy_press_cancels_on_drag_focus_loss_and_resize`, `approval_and_command_presses_do_not_activate_beneath_the_drawer` |
