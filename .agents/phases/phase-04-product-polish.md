# Phase 04 — Product polish and extensibility

| Field | Value |
| --- | --- |
| Status | Active; stages 1–9 complete; stage 11 composer menu and Drawer in progress; stages 12 streaming continuity and 13 chrome diet complete; stage 10 branding remains |
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
   [SPK-1–SPK-3](../specs/conversation-picker.md) cover lazy JSONL creation, restoration feedback,
   message-local retry/edit-retry, hover Copy and the session picker. Offline workspace tests,
   three-width frames and PTY smoke passed. It consumes Phase 02's journal/context APIs;
   compaction remains owned there. Text selection and main-agent approval refinements are stage 4;
   per-turn usage presentation remains a follow-up.
   New conversation reuses SPK-2's replacement owner and lazy storage.
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
7. **Responsive native math — approved and complete.** FR-5, MD-4/MD-5 and
   [PRE-1–PRE-4](../specs/render-preparation.md) deliver coalesced frames, reusable geometry and
   owned preparation. [MTH-1–MTH-5](../specs/math-layout.md) own terminal output,
   native conversation math, atomic delimited-source copy, capability fallback and clipping.
   All 61 reply formulas traverse the real child at three widths. The direct-Kitty CLI check
   covers real mouse copy, resize, overlays and clean exit; the user approved its live appearance
   on 2026-09-06. Fifteen actual workspace frames cover native reply, selection, source, tables
   and clipping at [120](../../crates/plexmaton-tui/frames/math/reply-120.svg),
   [88](../../crates/plexmaton-tui/frames/math/reply-88.svg) and
   [60](../../crates/plexmaton-tui/frames/math/reply-60.svg), with matching cases alongside.
   The user also approved ENT-4's disclosure-only tool click on 2026-09-06; drag/keyboard
   selection remain explicit. Its three-width evidence lives in
   [transcript entry](../specs/transcript-entry.md#evidence). Workspace tests/check/Clippy,
   dependency/document gates, both PTY smokes and the real Kitty fixture passed.
   Source reveal, tmux sizing, broader font/terminal fidelity and physical-terminal latency remain
   explicit follow-ups, not a claim of complete KaTeX parity. The consumed stage plan is removed.
8. **Agent Skills — complete.** [SKL-1–SKL-6](../specs/agent-skills.md) implement project model
   selection, three skill roots, bounded metadata and resource reads, model activation and durable
   explicit invocation including edit/retry. The [source comparison](../spikes/agent-skills/README.md)
   records the design evidence. 840 Rust tests and 17 script regressions pass; formatting,
   all-target compilation, workspace Clippy, crate graph, file length, citations, document budgets,
   typos and machete pass. Offline dependency audit passes with the existing hashbrown/syn duplicate
   warnings. Both offline PTY smokes pass without model requests. Reviewed skill notice frames at
   [wide](../../crates/plexmaton-tui/frames/skill-diagnostic-wide.txt),
   [medium](../../crates/plexmaton-tui/frames/skill-diagnostic-medium.txt) and
   [narrow](../../crates/plexmaton-tui/frames/skill-diagnostic-narrow.txt) widths.
   CPL-3 owns required skill retention during compaction. Live model behavior and performance remain
   unverified. JRN-3 preserves readable `2026-09-04` history alongside new `2026-09-05` files.

9. **Composer skill picker — complete.** [SKP-1–SKP-4](../specs/composer-menu.md) put `$` discovery
   above the primary input, with keyboard/pointer completion and selected-name ownership through
   input return and edit/retry. Literal variables, currency and prose remain text. 853 Rust tests,
   17 script regressions, workspace compilation/Clippy, formatting, crate graph, file length,
   citations, document budgets, typos and machete pass. The revised real PTY smoke exercises `$`,
   filtering, Enter/Tab completion and Escape without model requests or an empty journal; the
   footer smoke also passes. Reviewed actual buffer exports at
   [wide](../spikes/agent-skills/frames/skill-picker-wide.svg),
   [medium](../spikes/agent-skills/frames/skill-picker-medium.svg), and
   [narrow](../spikes/agent-skills/frames/skill-picker-narrow.svg) widths. No dependency or journal
   schema change was needed for the picker; live model behavior remains unverified.

10. **Branding.** A rounded-square frame and circular gradient center form the user's visual
    reference. Palette, pure Ratatui drawing and a visible-only animation clock are separate
    responsibilities. Static geometry/color previews precede motion; no script reruns per logo frame.
11. **Composer menu and Drawer — in progress.** A Command is a slash command only, typed into the
    conversation it addresses and run from there with a captured target: `/new`, `/resume`,
    `/compact` and `/permissions` for the Session. The `$` picker becomes the composer menu and
    serves `/` too. The Drawer, pulled from the top edge by `Ctrl-P`, holds what outlives a
    Session: Configuration, and Project and User permissions; `/config` and the three-second
    palette hint cease to exist. The user set the vocabulary and the Drawer's geometry on
    2026-09-06 and, the same day, decided that conversations and Session permissions are typed
    where the user types, which moved Conversations out of the Drawer after it had landed there
    with [DRW-1–DRW-4](../specs/drawer.md). Real frames replace the sketch slice by slice under
    the [plan](../plans/phase-04-stage-11-composer-menu-and-drawer.md). Rejected: settings as
    slash commands, which put workspace pages in the composer and made its title lie about the
    addressee; and conversations as a Drawer page, which hid what users type by habit behind a
    chord.

12. **Streaming continuity — complete.** MD-4, PRE-3/PRE-4, FR-3/FR-4 and MTH-5 cover retained text
    prefixes, exact painted-source copy, explicit cached refusal identity and bounded capture.
    ENT-2 tool transitions require current preparation; TR-1 shares feedback measurement and paint.
    Validation after rebasing onto `2febe5b` passed 1,089 Rust tests, 22 Python tests and all three
    terminal smokes, with workspace check/Clippy and the repository hook. Actual streaming and tool-transition frames
    at 120/88/60 columns were inspected in system-temporary storage. The consumed plan is removed.
    An earlier partial-pipe fixture timeout remains unexplained; isolated, full-fixture and later
    workspace runs passed without relaxing its deadline. Current release timings and saturated
    physical-terminal streaming remain unmeasured. A separate literal-TeX wireframe awaits the
    user's choice; unfinished-math grammar and the UI/UX contract are unchanged.

13. **Chrome diet — complete.** The conversation column has no box: the transcript runs into
    the composer's top rule and ends with its activity line, which also carries the selection
    note and the attention pill; the composer sits between two rules that carry only the
    addressee and the reasoning effort, grows to a third of the terminal and walks a taller
    draft with `↑`/`↓` and the wheel; menus are a titled rule and rows the composer's top rule
    closes. Decided by the user on 2026-09-06 from hand-composed frames; the real frames are the
    regenerated composition fixtures, the `composer-grown-*` and `composer-windowed-*` crops and
    the Markdown SVGs. The Drawer, the rail and the strips keep their boxes. Rejected: the box,
    chrome that said nothing; and current work on the composer's rule, which mixed the agent's
    doing with the user's typing.

The status-line adapter does not bundle the approval repair or logo animation.
Other roadmap work in performance, math and extensibility receives a stage when its evidence is
ready; this phase opening does not claim those capabilities have started.

## Exit gate

| Criterion | Evidence expected |
| --- | --- |
| Presentation does not own session semantics | Removing a script/logo leaves journal, provider requests, accounting and recovery unchanged |
| Facts stay honest | Resume preserves durable snapshot fields; unknown values remain unknown; context occupancy and cumulative traffic differ explicitly |
| External execution is bounded | Real-process tests prove timeout, replacement, output limits, descendant cleanup and shutdown |
| UI remains usable at every supported size | Wide/medium/narrow frames reviewed, including approval, pending quit, missing script, clipping, the Drawer, the composer menu and the column without boxes |
| Updates have an owner | No per-render subprocess or full-history accounting; ticks exist only for explicitly enabled visible work |

Rejected: importing stale branch implementation over current main, and opening a generic plugin
framework to run one configured presentation command.
