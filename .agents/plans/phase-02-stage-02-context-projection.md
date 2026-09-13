# Plan — Phase 02 stage 2, context projection and rewind

| Field | Value |
| --- | --- |
| Phase | [Phase 02 — Durable sessions and context](../phases/phase-02-durable-sessions.md) §scope 1–3 |
| Contract | [Conversation tree](../specs/conversation-tree.md) TRE-1–TRE-8; JRN-1–JRN-8, TIM-1, CPL-5, CMC-2, INV-1/INV-3/INV-6 |
| Status | Implementation and local acceptance complete; slice 10.9 PR CI pending |
| Review | The user will test the native executable directly; no additional mockups or preview artifacts |

## Outcome

Inspect a conversation tree, revisit a message and continue on a fresh branch while preserving original history, checkpoint ancestry and recorded effects. `/tree` and `/rewind` open one dedicated modal.

## Slices

Slices 1–9 are complete; TIM, JRN, PRV, BUD and CPL own their evidence. Rewind slices 10.1–10.7 are implemented and locally verified; TRE-1–TRE-8 own the named proofs. Search and generated branch summaries are omitted. Slice 10.9 awaits PR CI.

## Local acceptance

- Full core, agent, runtime and store suites: 453 tests passed.
- Full TUI suite: 453 tests passed, including 21 tree tests.
- Full CLI suites: 166 tests passed.
- Affected crates, all-target Clippy with `-D warnings`: passed.
- Python gate/fixture tests: 38 passed.
- Real PTY lifecycle and tree journey: passed. Six loopback model requests prove exact destination context, original-branch return, non-default selected-head restart and a command effect executed only once.
- Native wide, medium, narrow and minimum-size frames inspected locally. The user owns direct interactive acceptance.

These are local results, not GitHub CI results. Fixtures never contact a real model endpoint.

## Order and why

Publish `feat/conversation-rewind` as the requested PR, let CI verify the branch, and address only actionable failures. Merge requires explicit authorization. After an authorized merge, sync main and retire the task resources under [Git workflow](../standards/git-workflow.md); then delete this consumed plan and update the phase and roadmap together.

## Deliberately not in this plan

Search, generated branch summaries, physical garbage collection, cross-session mail, MCP, new provider transports, themes or general layout changes. No further preview generators, design mockups or handoff files.
