# Phase 04 — Product polish and extensibility

| Field | Value |
| --- | --- |
| Status | Active; stage 1 complete; stage 2 session interaction in progress |
| Parent roadmap | [Plexmaton Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Depends on | Phase 01 interaction ownership and Phase 02 acknowledged journal/accounting/budget projections |

## Outcome

A legible default terminal workspace with user-owned presentation customization. Status scripts
consume typed snapshots; the application retains session, input, approval and rendering ownership.
Visual changes are reviewed against real frames before the contract adopts them.

## Scope and sequence

1. **Configurable status line — complete.** [STL-1–STL-4](../specs/status-line.md) define the versioned
   snapshot, owned command, inert styled output and user-approved variable-height footer. The pastel
   Powerline example includes rainbow path components and omits unavailable statistics. Workspace
   tests, two independent code reviews and the real PTY status-line smoke passed without model calls.
2. **Session interaction — in progress.** [Stage 2](../plans/phase-04-stage-02-session-interaction.md)
   has verified restoration feedback, message-local retry/edit-retry and the session picker;
   lazy automatic creation remains.
   It consumes Phase 02's journal/context APIs; compaction remains owned there. Attention/approval
   layout and per-turn usage presentation remain separate follow-ups.
3. **Branding.** A rounded-square frame and circular gradient center form the user's visual
   reference. Palette, pure Ratatui drawing and a visible-only animation clock are separate
   responsibilities. Static geometry/color previews precede motion; no script reruns per logo frame.

The status-line adapter does not bundle the approval repair or logo animation.
Other roadmap work in performance, math and extensibility receives a stage when its evidence is
ready; this phase opening does not claim those capabilities have started.

## Exit gate

| Criterion | Evidence expected |
| --- | --- |
| Presentation does not own session semantics | Removing a script/logo leaves journal, provider requests, accounting and recovery unchanged |
| Facts stay honest | Resume preserves durable snapshot fields; unknown values remain unknown; context occupancy and cumulative traffic differ explicitly |
| External execution is bounded | Real-process tests prove timeout, replacement, output limits, descendant cleanup and shutdown |
| UI remains usable at every supported size | Wide/medium/narrow frames reviewed, including approval, pending quit, missing script and clipping |
| Updates have an owner | No per-render subprocess or full-history accounting; ticks exist only for explicitly enabled visible work |

Rejected: importing stale branch implementation over current main, and opening a generic plugin
framework to run one configured presentation command.
