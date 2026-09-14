# Phase 03 — Durable collaboration

| Field | Value |
| --- | --- |
| Status | Active; the backend is built and has no production caller, so no delegated child can be created today |
| Parent roadmap | [Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Depends on | JRN-4/JRN-7, LIVE-1/LIVE-3 and the existing provider context boundary |
| Inherits | Phase 02 JSONL recovery (JRN-1–JRN-8), four provider dialects (PRV-1–PRV-7), compaction (CPL-1–CPL-8), permissions (PER-1–PER-10/PGR-1–PGR-5), instructions (AGI-1–AGI-5) and durable tree navigation (TRE-1–TRE-8); unproven acceptance: real CLI kill→resume with equal projections (JRN-4/JRN-5), and pending-request readmission (APV-6); JRN-5 currently cancels orphaned calls on recovery |

## Outcome

Independent runners reuse the existing agent loop and session journal. The main agent dispatches
children using configured model/effort choices and communicates through typed mail. The user
observes that mail and may stop work; direct input opens only after explicit durable handoff.
Default resume activates only the main runner, retaining child history and canonical references.

## What exists, and what stops it running

Every part below is implemented and tested at its own boundary. None of them is reachable from the
executable, so the roster is empty in a real session and `delegate` is absent from the model's
tools. The right-hand column is the whole gap, and no part of it depends on another.

| Part | Owns | Missing |
| --- | --- | --- |
| Ledger | [COL-1–COL-5](../specs/collaboration-ledger.md): one bounded log for mail, task updates and Handoff; retries reconcile to the original item; crash cuts recover | — |
| Inclusion | [CIN-1–CIN-4](../specs/collaboration-inclusion.md): frozen eligible prefix, canonical session references, bounded resolution before dispatch | — |
| Control | [COL-3](../specs/collaboration-ledger.md): single controller, authority-scoped reservations, quiescent durable handoff, crash recovery | Requested compaction and tree mutation stay ungated; they must not reach a Main-controlled child surface until product routing owns them |
| Child bootstrap | [CHB-1–CHB-3](../specs/delegated-bootstrap.md): fresh and resumed constructors, capability floor, root-only resume | `DelegatedChildFactory::new` has no production caller — every construction site is a test |
| Scheduling | [SCH-1–SCH-4](../specs/owned-scheduling.md): bounded owners, separate normal/control/update lanes, Stop under backpressure, joined shutdown | — |
| Wake and mail projection | [SCH-5](../specs/owned-scheduling.md), [CMP-1](../specs/collaboration-mail-projection.md): coalesced hints, branch-local boundary reread, attributed Incoming/Sent snapshots | `OwnedCollaboration::next_activity` has no production consumer, so a child's activity cannot reach the TUI |
| Tool grammar | [CTL-1–CTL-2](../specs/collaboration-tools.md): four typed schemas, authenticated ingress, recoverable provisioning | `NativeToolCatalog::with_main_collaboration` has no production caller — the CLI opens a `Full` catalog, so the model never sees `delegate` |
| Provider | Four dialects encode every other atom | All four refuse a collaboration atom ([PRV-1](../specs/provider-adapter.md)), so a child's first request cannot be built. This is a decision, not a gap: see the rule and its rejected alternatives |
| Control view | [CCV-1–CCV-4](../specs/child-control-view.md): controller presentation, composer gate, passive acknowledgment | Production source; the fixtures supply control facts by hand |
| Roster | Ordering by attention, the ruled break, `Ctrl-B`, width-dependent docking | Nothing appends `AgentCreated` for a child to the parent journal, so the panel has no data to order |

Semantics stay in the agent crate, storage in session-store, orchestration in runtime; no second
agent engine or journal format is planned.

## Exit gate

| Requirement | Evidence required |
| --- | --- |
| Accepted mail survives process death | Real file crash cuts, retry reconciliation and uncertain-write freeze |
| Cross-session state has one authority | Equal live/reopened ledger and inbox/Attention projections |
| Control is exclusive and handoff is explicit | Active/idle input gates, unbound-child fail-closed behavior, stale-ticket refusal, quiescent Handoff and CHB-1 capability stability across Handoff |
| Primary remains responsive | `sch_2_owner_multiplexes_two_independent_runners` and `sch_2_stop_completes_while_the_update_lane_is_saturated` prove the backend owner; provider mail wake and product/TUI saturation remain unproven |
| Provider provenance is explicit | Fixtures for all four dialects; mail reaches a model with its sender named |
| User can inspect, stop and continue after handoff | Canonical journey and reviewed frames at three widths |

## Outstanding

- Process death inside the collaboration-log/child-journal provisioning window.
- A canonical cross-session Attention source. Background requests must not be inferred from mail or
  task state.
- User visual acceptance of the control view and the roster against a real delegated child.
- Reference retention across branch, export and delete, settled before those operations are exposed
  together with collaboration.
- Power-loss durability, which JRN-4 does not promise. Admission uses the same process-death
  boundary; stronger storage semantics need their own contract change.
