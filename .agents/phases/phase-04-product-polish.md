# Phase 04 — Product polish and extensibility

| Field | Value |
| --- | --- |
| Status | Active; stages 1–6 complete; stage 7 rendering performance delivered, native math foundation pending |
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
2. **Session interaction — complete.** JRN-4/JRN-7/JRN-8, SEL-7 and
   [SPK-1–SPK-3](../specs/session-picker.md) cover lazy JSONL creation, restoration feedback,
   message-local retry/edit-retry, hover Copy and the session picker. Offline workspace tests,
   three-width frames and PTY smoke passed. It consumes Phase 02's journal/context APIs;
   compaction remains owned there. Text selection and main-agent approval refinements are stage 4;
   per-turn usage presentation remains a follow-up.
   `/new` reuses SPK-2's replacement owner and lazy storage. Its palette row was inspected at
   [wide](../../crates/plexmaton-tui/frames/command-palette-wide.txt),
   [medium](../../crates/plexmaton-tui/frames/command-palette-medium.txt) and
   [narrow](../../crates/plexmaton-tui/frames/command-palette-narrow.txt) widths.
3. **Markdown transcript — complete.** [MD-1–MD-4](../specs/markdown.md) provide bounded assistant
   formatting while preserving journal source and virtualized scrolling. Reviewed
   [wide](../../crates/plexmaton-tui/frames/markdown-wide.txt),
   [medium](../../crates/plexmaton-tui/frames/markdown-medium.txt) and
   [narrow](../../crates/plexmaton-tui/frames/markdown-narrow.txt) frames.
4. **Text selection and local approvals — complete.** [SEL-1–SEL-7](../specs/selection-and-copy.md)
   provide cross-entry plain-text drags and auto-copy; the Copy icon retains source. ATT-1/ATT-3
   keep main approvals in their conversation, including after Esc, and advance pending cards in
   arrival order. Reviewed selection frames at
   [wide](../../crates/plexmaton-tui/frames/text-selection-wide.txt),
   [medium](../../crates/plexmaton-tui/frames/text-selection-medium.txt),
   [narrow](../../crates/plexmaton-tui/frames/text-selection-narrow.txt), plus the corresponding
   `native-approval-*` frames. 287 TUI tests, offline workspace tests, Clippy and terminal smoke
   passed. FR-4 records text-drag timings and the remaining cold-layout budget gap. Manual live
   model/terminal use remains unverified; no model requests were made by validation.
5. **Test-quality hardening — complete.** [Testing](../standards/testing.md) and
   [quality gates](../standards/quality-gates.md)
   cover gate integrity, readiness-driven smoke, blocking process fixtures, parser/cache witnesses,
   owned test directories and shared/cropped frame fixtures. A reproducible duplicate-descriptor
   regression also fixes JRN-4 writer-lock release. Workspace gates, 17 script regressions and
   offline PTY smokes passed; the runtime suite passed two extra consecutive runs after the fix.
   Warm debug terminal smoke measured 49.17 s before readiness-based waits and 1.98/1.90 s after;
   configured-footer smoke passed in 3.38/3.67 s. These local `/usr/bin/time -p` samples are not CI
   budgets; the old footer baseline failed a stale eager-file assertion and is not a speed comparison.
6. **Markdown styling — approved and complete.** [MD-5](../specs/markdown.md)
   defines Markdown-only pastel color/weight choices beside the unchanged status line.
   User-approved colors are the CLI's Markdown default; terminal chrome and the script are unchanged.
   All TUI/CLI targets, workspace Clippy and both offline PTY smokes passed.
   Reviewed real single-agent renderings at [120 × 40](../../crates/plexmaton-tui/frames/markdown-style-120.svg),
   [88 × 42](../../crates/plexmaton-tui/frames/markdown-style-88.svg) and
   [60 × 46](../../crates/plexmaton-tui/frames/markdown-style-60.svg), plus an 88 × 20 short viewport;
   the user opened and approved the 88-column sample. The SVG export models a dark terminal's ANSI
   slots and fonts, not their exact terminal configuration. Reproduce with
   `cargo run -p plexmaton-tui --example markdown_style_preview -- target/markdown-style`.
   Copy fragments, nested styles and cache invalidation are tested. No code syntax highlighter or
   new dependency was added; a rich-Markdown performance budget remains unmeasured.
   Single-agent samples do not prescribe an A2A layout. Notification and agent interaction changes
   are explicitly deferred by the user.
7. **Responsive native math — in progress.** [Stage 7](../plans/phase-04-stage-07-math-typesetting.md)
   delivers bounded streaming frames, shared wrap geometry and palette-independent heights.
   [FR-4/FR-5](../specs/frame-loop.md) own performance and source-preservation evidence.
   Native math admission, owned preparation/transport and atomic formula interaction remain.
8. **Branding.** A rounded-square frame and circular gradient center form the user's visual
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
