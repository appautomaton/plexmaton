# Phase 03, stage 7 — Product integration

| Field | Value |
| --- | --- |
| Phase | [Phase 03](../phases/phase-03-collaboration.md) |
| Contract | COL-3, CHB-1–CHB-3, SCH-1–SCH-5, CIN-2–CIN-4, CCV-1–CCV-4, ENT-1/ENT-3, JRN-5/JRN-7, ATT-1–ATT-3, INV-7 |
| Status | Active; 7 of 8 slices complete. Next: slice 8 (complete journey and consistent names) |

## Outcome

Finish the existing collaboration implementation; retain its ledger, inclusion, ingress, runners
and committed smoke refactor. The [phase](../phases/phase-03-collaboration.md#current-evidence)
owns the baseline. These eight pending slices replace the former unfinished control/activation slice.

M is one main boundary; L crosses boundaries and needs component plus executable evidence.
Sizes describe scope, not duration. Resolve Read IDs through
[mechanism routing](../phases/phase-03-collaboration.md#mechanism-routing);
source paths are relative to the named crate's src directory.

## Slices

1. **Ordered publication — M; complete.**
   Read: JRN-7; ENT-1/ENT-3.
   Start at runtime `project_delegated`, `runtime/transition.rs` and
   `runtime/tests/persistence/barriers.rs` (`StoreControl`).
   Hold a journal acknowledgement, interleave delegated facts, then release it; establish a failing
   schedule before repairing publication; the trace does not establish its cause.
   **Closes:** tier 2 proves consecutive, exact-once publication and the disposition of deferred
   facts after cancelled waits and failed/uncertain appends. No uncommitted fact escapes. The
   regression fails on old code; repeated green smoke runs are not a substitute.

2. **Owned child interruption — M; complete.**
   Read: LIVE-3; SCH-2/SCH-4; INV-7.
   Start at CLI `interaction.rs`, `input.rs`, `collaboration.rs` and runtime
   `owned_collaboration/lifecycle.rs`. Reproduce focused-child Ctrl-C, then address its real
   owner; retain settlement while continuing to pump workspace activity.
   **Closes:** running/idle/repeated Stop and missing or resumed runners cannot exit the CLI or
   interrupt the root. Active work joins; accepted mail settles; late wake cannot restart stopped
   work. Tier 2 covers cancellation; a paused-provider PTY proves Stop and root continuation.

3. **Passive child history after resume — M; complete.**
   Read: CHB-3; INS-1/INS-4/INS-6.
   Start at CLI `collaboration.rs::{restore,replay_children,sync_roster}`, session startup and
   roster/inspector registration. Make the persisted child projection selectable without starting
   a runner. Missing, locked or corrupt history needs an explicit unavailable state.
   **Closes:** keyboard and pointer open the exact child's work after restart, at 120/95/60 columns.
   Close/reopen retains reading anchors with zero requests/effects. Test unavailable-history paths.

4. **Durable shared-entry placement — L; complete.**
   Read: ENT-1; CIN-2; JRN-3/JRN-5.
   Start at CLI `collaboration/projection.rs`, session replay and inclusion references.
   Define placement from canonical durable links; preserve one source of truth. If old journals
   lack an anchor, document a lossless compatibility policy rather than inventing original order.
   **Closes:** tier 2 compares ordered live/reopened semantic items on selected branches; repeat
   reopen and subsequent child activation duplicate nothing. The PTY verifies task/mail positions
   among conversation work and before the restoration confirmation. No wall-clock merge.

5. **Execution ownership after Handoff — L; complete.**
   Read: COL-3; SCH-3/SCH-4; CHB-1/CHB-2.
   Start at `owned_runner`, `owned_collaboration`, `collaboration_ingress` and
   `runtime/collaboration.rs`. Existing Handoff settles the ledger; add a bounded authenticated
   user-input lane to the one owned child runtime, including explicit cold activation.
   **Closes:** no direct input before durable transfer; afterward input reaches only that child.
   History/capabilities survive, Main admission and stale tickets fail, cancellation/duplicate
   transfer/write failure settle safely, and shutdown joins ownership. Default resume executes
   nothing. Prove active/idle transfer and reopened User control at tier 2 before enabling UI input.

6. **Controller projection and child input — M; complete.**
   Read: CCV-1–CCV-4; INS-5/INS-7; UI/UX delegated control.
   Feed revisioned snapshots from the authenticated owner, project a distinct Handoff entry, and
   route focused input/refusals to slice 5. Retain exact undelivered drafts. Presentation grants no
   authority; input must not appear before the execution route works.
   **Closes:** Main/pending/unknown stay locked; acknowledged User control enables input without
   focus theft or auto-send. The existing Main `handoff` tool drives the PTY transition, followed
   by a child-addressed message/response. Verify Stop hints, capabilities, history and selection/
   scroll preservation at three widths. No new Handoff shortcut is required.
   **Evidence:** owner/CLI tests cover pending, rollback, synchronous retained product input, cold
   activation off the terminal path, exact draft return and both Handoff rows;
   `scripts/smoke-delegate.py` drives the real tool, child-only response, Stop and passive
   User-control resume in saved 120/95/60 frames.

7. **Canonical background requests — L; complete.**
   Read: ATT-1–ATT-3; APV-4/APV-6; COL-1–COL-4; CMP-1/CMP-2.
   Start at producer-owned pending state, CLI child-event forwarding and the TUI Attention queue.
   The producer journal owns pending/resolved truth and decision authority. Admit bounded,
   authenticated request/resolution references through the collaboration item log; the root
   projection owns no lifecycle state. Do not bypass the log or duplicate the pending-state machine.
   Mail, task progress and failure cannot substitute for a request.
   **Closes:** tier 2 verifies the live owner-addressed decision route and passive, effect-free
   reopen of request identities/state over the same validated prefix. Recovered orphan requests
   remain inert; restored decision admission/current-policy checks belong to stage 8.3. Live
   acknowledgement differs from resolution; wrong-owner/stale decisions fail. Product component
   tests prove the exact 120/95/60 request, preserved root focus/draft, user navigation and producer
   resolution. Stage 8.3 owns the first CHB-1-supported child request source, runtime/CLI
   continuation and real PTY witness. Rejected: a fixture-only policy or false `send_mail`
   capability bypasses product authority.

8. **Complete journey and consistent names — M; after 1–7.**
   Read: UI/UX vocabulary; ENT-1; the phase exit table.
   Use existing display labels (Plexmaton, Delegated N) in roster, titles and correspondence;
   typed IDs/selectors stay unchanged. Extend `smoke-delegate.py` with task
   update, Stop, Handoff/input, resumed-child browsing and no unintended wake.
   **Closes:** exact addressed outcomes, durable facts and the complete 120/95/60 flow pass,
   including returning to the root at 60. Hold child work while proving root responsiveness;
   the fixture must serve independent requests so it cannot itself serialize both agents.
   Inspect native frames as well as markers. Stage 8 still gates phase closure.

## Order and why

Run in number order, one slice in progress. Publication/Stop protect later work; passive recovery
precedes placement; runtime authority precedes editable UI.

Use the phase's [coordinator protocol](../phases/phase-03-collaboration.md#coordinator-protocol)
for delegation packets, evidence, continuation and authorization. It survives this plan's deletion.
Apply its [Luna context and slot lifecycle](../phases/phase-03-collaboration.md#luna-context-and-slot-lifecycle)
at every assignment: measure the worker's usable context, checkpoint, retire and verify capacity.

## Deliberately not in this plan

[Stage 8](./phase-03-stage-08-recovery-acceptance.md) owns recovery acceptance and phase closure.
The phase owns deferred scope. No UI redesign or capability expansion.
