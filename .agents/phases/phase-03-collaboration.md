# Phase 03 — Durable collaboration

| Field | Value |
| --- | --- |
| Status | Active; durable admission complete; owned scheduling next |
| Parent roadmap | [Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Depends on | JRN-4/JRN-7, LIVE-1/LIVE-3 and the existing provider context boundary |

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
2. **Owned scheduling and inclusion.** Two scripted agents exercise asynchronous delegation,
   bounded runners, accepted/included/completed distinctions, reserved control capacity, wake/stop
   policy and joined cancellation. Session inclusion records reference canonical items and bind the
   pre-turn amendment barrier. Branches retain references; replay never repeats effects.
3. **Provider and projection boundary.** Decide an explicit mail representation for every supported
   dialect before advertising collaboration. A typed internal atom does not establish provider
   acceptance. Unsupported representation is a typed refusal; synthesized user input is excluded.
   Integrate a thin inbox/Attention projection and explicit stop/amend intents early enough to test
   ownership, then review wide, medium and narrow frames under the unchanged UI/UX contract.
4. **Product integration.** Wire native delegation/mail tools and the interaction journey, prove
   responsiveness under saturation, then complete layout and interaction polish.

Stages 2–4 remain unimplemented gates; each receives a sliced plan when its work starts. Initial
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
corpus/dependency gates passed. CI and runtime/provider/UI integration remain unverified.

Power-loss durability is not promised by JRN-4. Admission uses the same process-death boundary;
stronger storage semantics require a separately justified contract change. Phase 02 branch
interaction and MCP may proceed independently, but collaboration activation must settle reference
retention across branch/export/delete before exposing those operations together.
