# Phase 03, stage 8 — Recovery acceptance and closure

| Field | Value |
| --- | --- |
| Phase | [Phase 03](../phases/phase-03-collaboration.md) |
| Contract | JRN-3–JRN-7, COL-3–COL-5, CIN-2–CIN-4, CHB-1–CHB-3, SCH-2–SCH-5, APV-6, ATT-1–ATT-3, PRV-1 |
| Status | Active; 2 of 4 slices complete. Next: slice 3 (pending approval readmission) |

## Outcome

Close Phase 03 against both the executable journey and the inherited durability/control evidence.
A green graceful-exit smoke does not establish process-kill recovery. Every required row of the
[phase exit gate](../phases/phase-03-collaboration.md#exit-gate) needs current evidence; an unresolved
row keeps the phase open unless the user explicitly changes its scope.

The phase's [coordinator protocol](../phases/phase-03-collaboration.md#coordinator-protocol) applies.
Its [Luna context and slot lifecycle](../phases/phase-03-collaboration.md#luna-context-and-slot-lifecycle)
also applies when rotating workers between recovery cases.
Load only the current slice's owners and tests. M/L are scope sizes, not time estimates.
The existing real-file tests are the starting point; add the missing boundary witness rather
than duplicating their entire matrices.

## Slices

1. **Provisioning across process death — L; complete.**
   `provisioning_process_death_recovers_one_exact_passive_child` kills the real child process after
   canonical creation, child-journal creation, runner registration and acknowledged mail. Every
   cut preserves the target, retry receipts and mail, then explicitly recovers one passive child.
   Lower tiers retain the exhaustive uncertain-write, corruption and provenance refusals.

2. **CLI kill → resume with equal projections — L; complete.**
   `scripts/smoke-delegate.py` kills the actual CLI while idle, during a paused child request and
   after pending Handoff projection but before its durable mutation. Exact bytes, selected ancestry,
   task/control/correspondence projections, sanitized request history and a synchronized tool
   invocation trace remain equal on passive pointer, keyboard and repeated resume. No provider or
   tool is replayed; one explicit root turn
   receives the recovered task/mail context. The pending cut reopens under Main and appends exactly
   one interruption, one cancelled call and one `process_died` terminal. Lower-tier JRN-4 tests own
   torn-tail permutations; actionable approval readmission remains slice 3.

3. **Pending approval readmission — L; pending; after 2.**
   Read [tool-admission](../specs/tool-admission.md) APV-6,
   [session-journal](../specs/session-journal.md) JRN-5 and
   [permission-policy](../specs/permission-policy.md) PER-1/PER-9.
   This is an inherited contract boundary, not evidence supplied by child Stop.
   Current JRN-5 cancels orphaned calls; APV-6 requires fresh admission/current policy when a
   persisted pending request is restored. First pin the supported restoration/explicit-continuation
   action with a focused test. Preserve effect-free replay and never treat a saved approval as authority.
   Establish the first production-supported child request source under CHB-1 and carry its
   continuation through the runtime and CLI owner route. The real PTY must show that request on the
   roster at 120/95/60 columns without taking focus, then open it only after user navigation.
   **Closes:** changing policy, revoking a grant or replacing the tool definition before reopen
   affects fresh admission; stale decisions cannot execute the old call; no effect runs merely
   because resume read a record. Prove the real supported flow through runtime, CLI and PTY
   boundaries.
   If the current cancellation rule leaves no such restoration flow, present that exact scenario
   and a concrete contract change for the user's decision. Do not silently count cancellation as
   readmission, invent automatic execution, or remove APV-6 from the phase to obtain a pass.

4. **Final acceptance and document closure — M; pending; after 1–3 and stage 7.**
   Assess every row of the phase exit table. Reuse spec-named tests for retry reconciliation,
   uncertain-write freeze, exclusive controller/stale tickets, unchanged child capabilities,
   all four provider encodings and scheduler backpressure. Add product-level evidence where the
   existing test only proves a backend boundary. A fixture that serializes paused child and root
   requests cannot prove concurrent responsiveness.
   Run all affected executable journeys once after integration, the Python suite and the
   [required quality gates](../standards/quality-gates.md). Prefer authorized CI for broad Rust
   checks; without publishing authorization run the necessary gates locally and mark CI unrun.
   Inspect real native wide/medium/narrow frames for working, stopped, handed-over and resumed
   children, including independent reading and returning to the root at 60 columns. Follow the
   [native review method](../spikes/kitty-native-preview/README.md); fixture frames alone do not
   establish production source authentication.
   **Closes:** current-code evidence, targeted review and required user visual acceptance are
   recorded; no unresolved required criterion is relabeled complete. Update root README,
   grammar/smokes and spec evidence where behavior changed; reconcile the touched quality-gates
   budget without raising it. Apply [corpus closure](../README.md#closing): remove consumed plans,
   update phase status/roadmap/inheritance together, and delete the phase only when its gate is met.
   Retain genuinely useful reference spikes. Do not commit, publish or merge without instruction.

## Order and why

Run after product integration; process death and current-policy restoration need a real route
whose results can be compared. The two-file window, CLI recovery and pending approval are separate
failure boundaries, so each gets its own witness and repair scope.

At each boundary record observed behavior, inferred cause and remaining uncertainty separately.
Use a barrier or fault injection to reproduce failures; retries, timeouts enlarged until green,
and repeated happy-path runs do not satisfy an unmet gate. If a fault requires a production change,
repair it in the owning slice and rerun only the checks affected by that change.

The final user review is a concrete set of native frames and behavior, prepared after independent
work is complete. While awaiting a required decision, preserve completed work and report the exact
blocked row; do not declare the phase closed. This planning task ends before any slice starts.

## Deliberately not in this plan

The [phase's deferred scope](../phases/phase-03-collaboration.md#deliberately-not-in-phase-03) remains
deferred. No power-loss/fsync promise, lossless export/import project, new provider or global/live
configuration change. Branch/export/delete reference retention is a prerequisite when those
operations are exposed with collaboration, not permission to build all three here.
