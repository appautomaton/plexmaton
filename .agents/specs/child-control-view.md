# Spec — Child control view

| Field | Value |
| --- | --- |
| Status | Implemented, wired and accepted. `scripts/smoke-delegate.py` drives Main Handoff, focused User child input and passive User-control resume at 120/95/60; authenticated Main and Stop presentation remain in the same journey |
| Owns | Revisioned child controller presentation, its composer gate and passive acknowledgment |
| Depends on | [UI/UX control and input](../ui-ux.md#delegated-conversation-control), COM-4, INS-5, INS-7, COL-3 and CHB-1 |
| Proven by | `plexmaton-tui::workspace::child_control::tests`, the runtime/CLI tests below and `scripts/smoke-delegate.py`; native review below |

## Invariants

**CCV-1 — Control arrives as an addressed snapshot.** A known child accepts monotonically increasing
control revisions; repeating the same revision and value is a no-op, while older or conflicting
revisions and primary/unknown-agent targets are refused without changing control. This presentation
snapshot grants no runtime authority and is not a conversation-journal event (COL-3).

**CCV-2 — Unknown, Main and pending control have no user input.** The inspector exposes a composer,
caret, draft edit or submission only for acknowledged User control with focus and sufficient room
(COM-4, INS-5, INS-7). Running, idle, completion, dismissal and reopen never infer a transfer. The
primary composer retains its normal size while a non-editable inspector holds focus.

**CCV-3 — Controller and capabilities stay visible.** A known delegated child retains its controller
indication and V1 read-only-file/no-shell profile through running, idle, pending and User states;
pending still names Main. Unknown control is visibly unavailable. Control chrome occupies no
semantic transcript rows and does not reorder mail, tools or artifacts (CHB-1, INS-6).

**CCV-4 — Acknowledgment is passive.** A control snapshot changes no focus, draft, completed selection,
history, viewport anchor or capability and submits no input; an input drag that becomes hidden settles
without copying. An addressed interrupt request remains independent
of control and changes no lifecycle/controller until its owner reports a result (COL-3, COM-4).

## Evidence

[Named proofs](../evidence/child-control-view.md), one row an invariant.

## Native review

The [Kitty fixture](../spikes/kitty-native-preview/README.md) uses the actual Workspace renderer.
The controller/profile line is fixed conversation chrome; the title prioritizes lifecycle and
Escape over counts when all cannot fit. The Ctrl-C Stop hint appears only while the child holds
keyboard focus and is running or waiting. Read-only file access and no shell describe the V1 profile;
typed mail remains available under CHB-1 and is not a filesystem write capability.

The user accepted the native structure and function on 2026-09-16. Minor visual refinements remain
separate work and must preserve these control and input boundaries.

The native fixture is not authenticated product ingress. The real PTY now proves exact-owner
controller projection, durable Handoff history, focused User input, capabilities and Stop-hint
behavior at all three widths, plus default-width Stop settlement. Canonical mail inclusion and
Attention use their own accepted production routes and evidence.
