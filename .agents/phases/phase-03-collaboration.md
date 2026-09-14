# Phase 03 — Durable collaboration

| Field | Value |
| --- | --- |
| Status | Active; the round trip was observed in the real executable — the main agent delegates, the child works and mails back, the root reads that mail and answers, and both the ask and the answer are on screen in both conversations and survive resume. That composition root has no tests: the observation is a demonstration, not evidence. The child's own transcript and control over it are not yet on screen |
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

`delegate` is in the model's tools, a call creates a real child, the child runs its task against a
real provider and mails its result back, the root takes a collaboration turn to read it, and the
letter is on screen in the child's window with its roster row counting it. What is still missing is
the rest of what the user would watch while that happens: the child's own transcript, and control
over it. The right-hand column is the whole gap.

| Part | Owns | Missing |
| --- | --- | --- |
| Ledger | [COL-1–COL-5](../specs/collaboration-ledger.md): one bounded log for mail, task updates and Handoff; retries reconcile to the original item; crash cuts recover | — |
| Inclusion | [CIN-1–CIN-4](../specs/collaboration-inclusion.md): frozen eligible prefix, canonical session references, bounded resolution before dispatch | — |
| Control | [COL-3](../specs/collaboration-ledger.md): single controller, authority-scoped reservations, quiescent durable handoff, crash recovery | Requested compaction and tree mutation stay ungated; they must not reach a Main-controlled child surface until product routing owns them |
| Child bootstrap | [CHB-1–CHB-3](../specs/delegated-bootstrap.md): fresh and resumed constructors, capability floor, root-only resume | — |
| Scheduling | [SCH-1–SCH-4](../specs/owned-scheduling.md): bounded owners, separate normal/control/update lanes, Stop under backpressure, joined shutdown | — |
| Wake and mail projection | [SCH-5](../specs/owned-scheduling.md), [CMP-1](../specs/collaboration-mail-projection.md): coalesced hints, branch-local boundary reread, attributed Incoming/Sent snapshots; the root reads its own inbox through a turn it admits itself, and each letter joins its sender's conversation and that sender's roster count | Queued and included read the same on screen; distinguishing them needs the CMP-2 session join, which no product surface calls yet |
| Tool grammar | [CTL-1–CTL-2](../specs/collaboration-tools.md): four typed schemas, authenticated ingress, recoverable provisioning; an assignment rebuilds the one sleeping child it names, or fails rather than recording work nothing will perform | Mail does not rebuild its recipient, so a letter to a child that is not running waits until an assignment wakes it. A collaboration tool call carries no retained invocation, so its row discloses nothing: the task or letter is readable only as the entry the projection draws beside it |
| Provider | Four dialects render mail as an attributed turn ([PRV-1](../specs/provider-adapter.md)) | Attribution is text the model reads, not a type the runtime enforces |
| Control view | [CCV-1–CCV-4](../specs/child-control-view.md): controller presentation, composer gate, passive acknowledgment | Production source, and the work itself: a child's window now shows what it was asked and what it answered, but nothing it did in between — its own events stay in its own journal under its own agent id. `HandoffCompleted` is also still drawn nowhere |
| Roster | Ordering by attention, the ruled break, `Ctrl-B`, width-dependent docking, lifecycle from the child's own events | A child is named `Delegated N` by the order it was created, not by what it does: `delegate` carries no name and the runtime picks the model |

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

- **One conversation, one way out — unproven.** `Record::emit` assigns a sequence and puts the
  envelope in the `Reaction` its caller holds, and a reaction raised inside a turn waits in
  `pending_commit` until the journal acknowledges the write, while `project_delegated` numbers and
  queues immediately. A later number can therefore reach the projection first, which drops the
  earlier one as stale. This is a reachable path, not a measured cause: no test constructs it, and
  the drops observed by hand were intermittent and never tied to a trace. Proving it needs a runtime
  test that holds a journal write open, projects a delegated fact across it, and asserts the
  published sequence stays monotonic. Rejected: the earlier reading that some reactions are
  *returned to the caller* — that exit no longer exists, so a fix written against it would change
  nothing.
- Process death inside the collaboration-log/child-journal provisioning window.
- A canonical cross-session Attention source. Background requests must not be inferred from mail or
  task state.
- User visual acceptance of the control view and the roster against a real delegated child.
- Reference retention across branch, export and delete, settled before those operations are exposed
  together with collaboration.
- Power-loss durability, which JRN-4 does not promise. Admission uses the same process-death
  boundary; stronger storage semantics need their own contract change.
