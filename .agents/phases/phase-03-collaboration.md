# Phase 03 — Durable collaboration

| Field | Value |
| --- | --- |
| Status | Active; turn inclusion complete; owned scheduling next |
| Parent roadmap | [Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Depends on | JRN-4/JRN-7, LIVE-1/LIVE-3 and the existing provider context boundary |
| Inherits | Phase 02 JSONL recovery (JRN-1–JRN-8), four provider dialects (PRV-1–PRV-7), compaction (CPL-1–CPL-8), permissions (PER-1–PER-10/PGR-1–PGR-5), instructions (AGI-1–AGI-5) and durable tree navigation (TRE-1–TRE-8); unproven acceptance: real CLI kill→resume with equal projections (JRN-4/JRN-5), and pending-request readmission (APV-6); JRN-5 currently cancels orphaned calls on recovery |

## Outcome

Independent session runners exchange bounded, attributed mail without blocking delegation on child
results. One collaboration item log owns cross-session facts, delegation amendments and objections;
inbox and Attention derive from it. The user can redirect a delegation without an agent silently
undoing that instruction. Existing session journals remain authoritative for their own transcripts.

## Scope and sequence

1. **Durable admission (complete).** [COL-1–COL-5](../specs/collaboration-ledger.md) own the pure
   collaboration ledger and exclusive file boundary. Mail, task amendments and objections share
   one bounded log; retries reconcile to the original item. Tier 1 tests cover user precedence,
   declared endpoints and limits; Tier 2 tests cover all byte cuts of an escaped, multibyte mail
   append, uncertain-write freeze, corruption, writer exclusion and process exit without Drop.
   Source review tightened final-tail recovery to reject impossible JSON and invalid UTF-8.
   This component starts no provider and changes no executable behavior.
2. **Turn admission and inclusion (complete).** [CIN-1–CIN-4](../specs/collaboration-inclusion.md)
   freeze the eligible prefix, retain canonical session references, and resolve bounded immutable
   context before dispatch. Tests cover amendment ordering, branch-local cursors, reopen between
   the two logs, both acknowledgement barriers, cancelled waits, unknown writes and preparation
   failure settlement. Two scripted LiveRuntime owners retain independent context and shutdown.
   All four production codecs explicitly refuse collaboration atoms; driver capability is checked
   before session mutation. This is a narrow backend path, not enabled product orchestration.
3. **Owned scheduling.** Compose bounded runners, normal/control/update channels, wake/stop policy
   and joined cancellation. The owner must intercept every turn-opening path before the pre-turn
   amendment guarantee applies to ordinary user input. Two scripted agents prove independent
   progress and stop under backpressure, with reserved completion/control capacity.
4. **Provider and projection boundary.** Decide an explicit mail representation for every supported
   dialect before advertising collaboration. A typed internal atom does not establish provider
   acceptance. Unsupported representation is a typed refusal; synthesized user input is excluded.
   Integrate a thin inbox/Attention projection and explicit stop/amend intents early enough to test
   ownership, then review wide, medium and narrow frames under the unchanged UI/UX contract.
5. **Product integration.** Wire native delegation/mail tools and the interaction journey, prove
   responsiveness under saturation, then complete layout and interaction polish.

Stages 1–2 are complete; stages 3–5 receive sliced plans when their work starts. Initial
admission adds no crate or external dependency: semantic reduction stays in the agent crate,
blocking storage in session-store, and later orchestration in runtime. LIVE-1 remains one runner
per agent; the collaboration owner composes runners rather than widening one into a global loop.

## Exit gate

| Requirement | Evidence required |
| --- | --- |
| Accepted mail survives process death | Real file crash cuts, retry reconciliation and uncertain-write freeze |
| Cross-session state has one authority | Equal live/reopened ledger and inbox/Attention projections |
| User amendments win and arrive in time | Both writer orders and pre-turn races, including context refusal |
| Primary remains responsive | Two scripted providers under mail/tool saturation and joined shutdown |
| Provider provenance is explicit | Fixtures for all four dialects; no synthesized user turn on resume |
| User can inspect and steer | Canonical journey and reviewed frames at three widths |

Validation is local: ledger tests, the session-store suite, affected all-target Clippy and static
corpus/dependency gates passed. GitHub PR checks own head-specific CI results; runtime/provider/UI integration remains unproven.

Power-loss durability is not promised by JRN-4. Admission uses the same process-death boundary;
stronger storage semantics require a separately justified contract change. Tree navigation is delivered;
MCP remains optional. Collaboration activation must settle reference
retention across branch/export/delete before exposing those operations together.
