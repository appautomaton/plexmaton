# Phase 03, stage 8 — Recovery acceptance and closure

| Field | Value |
| --- | --- |
| Phase | [Phase 03](../phases/phase-03-collaboration.md) |
| Contract | JRN-3–JRN-7, COL-3–COL-5, CIN-2–CIN-4, CHB-1–CHB-3, SCH-2–SCH-5, APV-6, ATT-1–ATT-3, PRV-1 |
| Status | Active; 1 of 4 slices complete. Next: slice 2 (CLI kill → resume) |

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
   Read [collaboration-tools](../specs/collaboration-tools.md) CTL-1,
   [ledger](../specs/collaboration-ledger.md) COL-4/COL-5 and
   [bootstrap](../specs/delegated-bootstrap.md) CHB-2/CHB-3.
   Start at runtime `collaboration_ingress/provisioning.rs`, child construction and session-store
   collaboration tests. Use a child process and explicit barriers around canonical creation,
   child-journal creation and runner registration; terminate the owned process at each cut.
   **Closes:** tier 4 proves acknowledgement/retry identity, one delegation and child journal,
   preserved accepted mail, and explicit recovery of a canonical-only child. Reopen starts no work;
   explicit target recovery cannot create a second child. Cover uncertain writes and refusal of
   malformed/mismatched provenance without erasing evidence. Keep barriers test-scoped and bounded.
   `provisioning_process_death_recovers_one_exact_passive_child` kills at all four boundaries,
   preserves target/retry/mail and recovers one passive child; lower tiers retain other failures.

2. **CLI kill → resume with equal projections — L; pending; after 1.**
   Read [session-journal](../specs/session-journal.md) JRN-4/JRN-5/JRN-7,
   [inclusion](../specs/collaboration-inclusion.md) CIN-2–CIN-4 and
   [bootstrap](../specs/delegated-bootstrap.md) CHB-3.
   Start at CLI session startup, collaboration restore and the shared PTY/provider fixtures.
   Kill the actual binary after acknowledged task/mail facts and during explicitly paused work;
   graceful quit is a separate case. Capture durable records and normalized semantic projections
   before/after restart, not only the presence of a few strings.
   **Closes:** selected branches, entry order, identities, correspondence and control agree across
   reopen, except documented recovery/lifecycle facts. Provider/tool counters prove zero replay
   effects; root continuation resolves the same canonical context, and passive child browsing
   starts no runner. Repeat resume adds no recovery debt or duplicated entries. Include a pending
   transfer cut and compare live/reopened ledger and passive request history. Recovered orphan
   requests remain inert here; actionable readmission belongs to slice 3.
   Keep sanitized fixture manifests, counters and failure traces in named task artifacts.

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
