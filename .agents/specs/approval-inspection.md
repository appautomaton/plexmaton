# Spec — Approval command inspection

| Field | Value |
| --- | --- |
| Status | Implemented; local validation complete; CI not run |
| Owns | Exact command inspection, modal controls and return to the pending approval |
| Depends on | APV-4, PER-5/PER-10, INV-2/INV-3/INV-11, SURF-1/SURF-3/SURF-4, ENT-4 |
| Proven by | Command admission round-trip and TUI workspace tests below |

## Invariants

**APD-1 — Inspection reads the admitted command.** `ToolDetail::Command` owns original shell source,
canonical working directory and timeout. Approval summaries may abbreviate or escape that source;
inspection and copy resolve the retained command by the current approval's agent and call identity.
The modal stores identities, never another command string. Tool-entry copy retains the complete
invocation context under ENT-4. Display makes controls inert and expands
tabs; original source remains unchanged for copying.

**APD-2 — Inspection is a read-only modal.** Clicking an available command summary or Ctrl-O opens
`CommandInspection` above the approval and below the Drawer. The registered modal owns scrolling,
blocks input below it, and preserves the card's stage, selection and draft. Esc or the top-right
close button returns to approval without deciding. Resolution or identity replacement invalidates
inspection; it cannot silently switch to the next command.

**APD-3 — Copy and close are explicit actions.** Plain `c` or the copy icon emits the complete
original command through the existing clipboard owner and leaves inspection open. The colored ×
closes it. Pointer actions require a matching, unchanged press/release and are cancelled by dragging,
keyboard focus or layer changes, terminal focus loss, or resize (INV-11). Approval digits have no meaning inside inspection.

## Evidence

| Invariant | Proven by |
| --- | --- |
| APD-1 | `cmd_1_command_detail_preserves_multiline_source_and_context`, `command_modal_copies_exact_source_and_returns_to_the_pending_approval`, `command_controls_are_visible_text_not_terminal_sequences` |
| APD-2 | `command_modal_copies_exact_source_and_returns_to_the_pending_approval`, `drawer_keeps_focus_when_the_inspected_approval_resolves`, `command_summary_press_pins_the_complete_request_identity` |
| APD-3 | `command_modal_copies_exact_source_and_returns_to_the_pending_approval`; `command_modal_copy_press_cancels_on_drag_focus_loss_and_resize`, `approval_and_command_presses_do_not_activate_beneath_the_drawer` |

## Review

`cargo run -p plexmaton-tui --example approval_preview -- target/approval-review` exports the actual
approval and modal frames at 120, 88 and 60 columns and checks exact command copy. Inspected approval frames at
[120](../../crates/plexmaton-tui/frames/approval/choices-120.svg),
[88](../../crates/plexmaton-tui/frames/approval/choices-88.svg), and
[60](../../crates/plexmaton-tui/frames/approval/choices-60.svg), plus command inspection at
[120](../../crates/plexmaton-tui/frames/approval/command-120.svg),
[88](../../crates/plexmaton-tui/frames/approval/command-88.svg), and
[60](../../crates/plexmaton-tui/frames/approval/command-60.svg).
The user approved the single-heading approval layout. The
[permission smoke](../../scripts/smoke-permissions.py) also exercises Ctrl-O, exact OSC 52 command
copy, Esc return and numbered scope confirmation through the executable. Permission
policy, offered scopes and the backend's correlated decision acknowledgement do not change.
