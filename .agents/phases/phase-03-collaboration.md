# Phase 03 — Durable collaboration

| Field | Value |
| --- | --- |
| Status | Active; Stage 7 Slice 6 control UI built and the roster/Attention redesign reviewed and landed, production activation blocked |
| Parent roadmap | [Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Depends on | JRN-4/JRN-7, LIVE-1/LIVE-3 and the existing provider context boundary |
| Inherits | Phase 02 JSONL recovery (JRN-1–JRN-8), four provider dialects (PRV-1–PRV-7), compaction (CPL-1–CPL-8), permissions (PER-1–PER-10/PGR-1–PGR-5), instructions (AGI-1–AGI-5) and durable tree navigation (TRE-1–TRE-8); unproven acceptance: real CLI kill→resume with equal projections (JRN-4/JRN-5), and pending-request readmission (APV-6); JRN-5 currently cancels orphaned calls on recovery |

## Outcome

Independent runners reuse the existing agent loop and session journal. The main agent dispatches
read-only children using configured model/effort choices and communicates through typed mail. The
user observes that mail and may stop work; direct input opens only after explicit durable handoff.
Default resume activates only the main runner, retaining child history and canonical references.

## Scope and sequence

1. **Durable admission (complete).** [COL-1–COL-5](../specs/collaboration-ledger.md) own the pure
   collaboration ledger and exclusive file boundary. Mail, Main-authored task updates and control
   Handoff share one bounded log; retries reconcile to the original item. Tier 1 tests cover
   declared endpoints and limits; Tier 2 tests cover all byte cuts of an escaped, multibyte mail
   append, uncertain-write freeze, corruption, writer exclusion and process exit without Drop.
   Source review tightened final-tail recovery to reject impossible JSON and invalid UTF-8.
   This component starts no provider and changes no executable behavior.
2. **Turn admission and inclusion (complete).** [CIN-1–CIN-4](../specs/collaboration-inclusion.md)
   freeze the eligible prefix, retain canonical session references, and resolve bounded immutable
   context before dispatch. Tests cover update/Handoff ordering, branch-local cursors, reopen between
   the two logs, both acknowledgement barriers, cancelled waits, unknown writes and preparation
   failure settlement. Two scripted LiveRuntime owners retain independent context and shutdown.
   All four production codecs explicitly refuse collaboration atoms; driver capability is checked
   before session mutation. This is a narrow backend path, not enabled product orchestration.
3. **Exclusive control and handoff (backend complete).** [COL-3](../specs/collaboration-ledger.md)
   now has single-controller reduction, authority-scoped execution reservations and permits,
   quiescent durable handoff, crash recovery, permit-retained runtime child turns, pre-mutation user
   input gates and the rendered user-approved direction. Requested compaction and tree mutations
   remain ungated and must not be exposed on Main-controlled child surfaces until product routing
   assigns their ownership. Authenticated product ingress and production composition remain
   pending.
4. **Read-only child bootstrap and root-only resume (backend complete).** [CHB-1–CHB-3](../specs/delegated-bootstrap.md)
   creates a fresh Conversation/runner with a
   configured model and allowed effort. V1 children have no delegation tool and no arbitrary shell
   or write capability; the execution boundary enforces that independently of prompt text. Reuse
   the journal grammar and retain a reference to canonical parent/delegation provenance. Default
   restore starts only the main runner; child history loads on demand and interrupted child work
   requires explicit rescheduling. Context inheritance, if added, must validate replay compatibility.
5. **Owned scheduling (backend complete).** [SCH-1–SCH-4](../specs/owned-scheduling.md) put the
   blocking collaboration file and each child runtime behind bounded owners with separate normal,
   control and update lanes. Runner capacity is independent of retained delegation history; exact
   requests, authority and reports survive canceled waits. Stop remains available under update
   backpressure, actor panic joins runtime cleanup, and Handoff validates before stopping and appends
   only after quiescence. Shutdown autonomously settles retained schedule/Stop/Handoff work before
   joining every runner and then the writer. Generic runtime admission cannot bypass owned turn or
   Handoff paths. Product ingress authentication remains a later boundary.
6. **Provider, wake and mail projection boundary (backend complete).** [SCH-5](../specs/owned-scheduling.md)
   coalesces content-free live-owner hints, rereads the child's branch-local boundary and canonical
   eligible prefix, and retains busy or cancelled scheduling without a second admission. Stale,
   stopped, handed-off and unsupported children fail closed. None of the current four dialects has
   a native typed-mail field, so all refuse before request construction rather than fabricate
   user/tool/assistant history or elevate peer mail to system authority. [CMP-1](../specs/collaboration-mail-projection.md)
   projects complete attributed Incoming/Sent snapshots through the canonical writer and survives
   reopen identically while a provable writer is open. Model inclusion remains a session-journal
   join; the canonical cross-session Attention source remains unproven. No new frames were needed
   for these backend boundaries.
7. **Product integration (backend complete through Slice 5).** [Stage 7](../plans/phase-03-stage-07-product-integration.md)
   closes cancellation-safe mutation ingress and owns authenticated native delegation/mail/Handoff
   grammar, recoverable child provisioning, repeated Main-to-child wake and the session-aware mail
   read model. Main identity, artifact origins and selected session snapshots are sealed to the
   owner capability and exact registered runtime instance; active children expose snapshots on a
   bounded inspection lane independent of Stop. Callers cannot supply endpoints or journals as authority.
   Slice 6 now implements [CCV-1–CCV-4](../specs/child-control-view.md) through explicit native Kitty
   fixtures, with user visual acceptance pending; production
   routing and journey evidence still wait for a provider with a safe typed collaboration
   representation. Process death inside the
   collaboration-log/child-journal provisioning window and a canonical Attention source remain
   unproven.

Stages 1–2 supplied the durable substrate. Stage 3's design replaces its task-control policy before
production child activation; it does not replace the log, context references or dispatch barriers.
Later stages receive sliced plans when they start. Semantics stay in the agent crate, storage in
session-store and orchestration in runtime; no second agent engine or journal format is planned.

## Exit gate

| Requirement | Evidence required |
| --- | --- |
| Accepted mail survives process death | Real file crash cuts, retry reconciliation and uncertain-write freeze |
| Cross-session state has one authority | Equal live/reopened ledger and inbox/Attention projections |
| Control is exclusive and handoff is explicit | Active/idle input gates, unbound-child fail-closed behavior, stale-ticket refusal, quiescent Handoff and CHB-1 capability stability across Handoff |
| Primary remains responsive | `sch_2_owner_multiplexes_two_independent_runners` and `sch_2_stop_completes_while_the_update_lane_is_saturated` prove the backend owner; provider mail wake and product/TUI saturation remain unproven |
| Provider provenance is explicit | Fixtures for all four dialects; no synthesized user turn on resume |
| User can inspect, stop and continue after handoff | Canonical journey and reviewed frames at three widths |

The current UI change passes 463 TUI library tests and four native-preview tests locally, with
all-target TUI Clippy, formatting, citations and file length passing. Kitty native readbacks cover
15 frames at three widths; normal exit restored terminal modes and released the alternate screen.
The final targeted Sol review found no remaining correctness blocker. User visual acceptance and
provider/UI product integration remain unproven.

The preceding backend validation recorded 234 Agent tests plus one compile-fail doctest, 67
session-store tests and 235 runtime tests (224 library and 11 integration), along with the workspace
and supply-chain gates. Those suites were not rerun for this UI-only change; current workspace-wide
and GitHub PR checks remain pending.

Power-loss durability is not promised by JRN-4. Admission uses the same process-death boundary;
stronger storage semantics require a separately justified contract change. Tree navigation is delivered;
MCP remains optional. Collaboration activation must settle reference
retention across branch/export/delete before exposing those operations together.
