# Phase 04 — Product polish and extensibility

| Field | Value |
| --- | --- |
| Status | Active; stages 1–9, 11–15 and 17–22 complete; stage 16 effort selection in progress; stage 10 branding remains |
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
11. **Composer menu and Drawer — complete.** A Command is a slash command only, typed into the
    conversation it addresses and run from there with a captured target: `/new`, `/resume` over
    saved conversations, `/compact` and `/permissions` for the Session, under
    [CMC-1 to CMC-3](../specs/composer-menu.md); SPK-1 to SPK-3 moved with the rows. The Drawer,
    pulled from the top edge by `Ctrl-P` under [DRW-1 to DRW-4](../specs/drawer.md), holds what
    outlives a Session: Configuration, and Project grants and configuration trust; `/config` and
    the three-second palette hint ceased to exist. One `PermissionPanel` serves both places and
    one `Pressed` slot serves every surface with rows (INV-11). The user set the vocabulary and
    the Drawer's geometry on 2026-09-06 and, the same day, decided that conversations and Session
    permissions are typed where the user types, which moved Conversations out of the Drawer after
    they had landed there. Rejected: settings as slash commands, which put workspace pages in the
    composer and made its title lie about the addressee; and conversations as a Drawer page, which
    hid what users type by habit behind a chord. Not done, each its own stage: `/model`, which
    needs the runtime to change a conversation's model mid-flight; a visible sign of messages
    queued for the next turn; the User rules snapshot, which the permission view does not
    project; and a loopback run of `/compact` through the executable, unproven in CMC-1.

12. **Streaming continuity — complete.** MD-4, PRE-3/PRE-4, FR-3/FR-4 and MTH-5 cover retained text
    prefixes, exact painted-source copy, explicit cached refusal identity and bounded capture.
    ENT-2 tool transitions require current preparation; TR-1 shares feedback measurement and paint.
    Validation after rebasing onto `2febe5b` passed 1,089 Rust tests, 22 Python tests and all three
    terminal smokes, with workspace check/Clippy and the repository hook. Actual streaming and tool-transition frames
    at 120/88/60 columns were inspected in system-temporary storage. The consumed plan is removed.
    Partial-pipe fixture readiness is sensitive to concurrent process startup: the production
    deadline can expire before the marker under load; serial fixture runs pass without changing
    that deadline. Current release timings and saturated physical-terminal streaming remain unmeasured. Stage 14 owns remaining formula-closure reflow
    reported during live use; retained preparation alone does not settle that behavior.

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

14. **Math LaTeX support — complete.** The user’s logits/softmax examples exercise accents,
    Chinese text and boxed mixed-language formulas. MTH-1–MTH-4 cover engine admission, real
    preparation and three-width projection of the four exact formulas. MD-3 keeps unfinished
    native math compact; its four token-stream collapses, up to seven rows, become zero in the
    exact fixture. After rebasing onto `58c3f66`, 1,117 Rust tests passed with serial scheduling,
    alongside workspace compilation/Clippy, 22 Python checks and three offline terminal smokes.
    Six MTH frames were regenerated and inspected with the current conversation/composer layout.
    Pixel-level streaming flicker remains unverified.

15. **Frozen Markdown prefix — complete.** MD-4 and PRE-1 reuse completed blocks through the
    owned worker while retaining a full parser pass. The formula-work witness prepares one formula
    instead of two; 72 suffix/width cases retain canonical rows and copy maps. Malformed hints and
    capacity pressure fall back without losing admissible source. Validation passed 1,117 workspace
    tests, compilation, Clippy and corpus gates. Seven paced Kitty deltas,
    250 ms apart, preserve formula copy, resize, Drawer interaction and clean exit at three widths.
    Reviewed math frames at [120](../../crates/plexmaton-tui/frames/math/logits-120.svg),
    [88](../../crates/plexmaton-tui/frames/math/logits-88.svg) and
    [60](../../crates/plexmaton-tui/frames/math/logits-60.svg), with pending frames alongside,
    were regenerated and inspected against `58c3f66`. TR-4 still moves bottom-aligned content as rows grow;
    this preparation optimization does not establish pixel-level flicker elimination.

16. **Reasoning effort — in progress.** The [stage plan](../plans/phase-04-stage-16-reasoning-effort.md)
    owns per-model allowed levels and the responsive RGB spectrum requested on 2026-09-06.
    [EFF-1–EFF-5](../specs/reasoning-effort.md) implement the live selector, idle driver replacement,
    shared RGB composer/selector colors and bounded four-shape max animation. Focused local checks
    and the user's terminal test close the stage; no further standalone previews are planned.

17. **Math projection — complete.** MTH-1–MTH-5 and MD-1–MD-4 cover joined short radicals,
    the exact multiline log-sum-exp loss and engine-owned compound root indices. Nested numerator
    scripts retain their ownership; script roots do not use full-size large glyphs. Local validation
    on `78c6e9c` plus this branch's changes passed 23 math tests, 389 TUI tests and three real-child
    native-reply tests. Math/TUI/CLI all-target Clippy, formatting, citations, frame references,
    file length and diff whitespace checks passed. The [MTH review evidence](../specs/math-layout.md#rendered-and-terminal-evidence) records
    inspected 120/88/60-column formula and index frames. Tall/script roots remain coarse;
    physical-terminal pixel fidelity and CI on this branch remain unverified.

18. **Approval inspection — complete.** [APD-1–APD-3](../specs/approval-inspection.md) cover exact
    command inspection/copy and return to the pending approval. INV-2/INV-3/INV-11 and PER-5/PER-10
    cover hover, numbered choices, painted-frame confirmation and stale/captured input guards.
    The user approved the single-heading layout; six linked wide/medium/narrow frames were inspected.
    On `2661aef` plus these changes, 629 unit tests and one doctest across core, agent, command and TUI
    passed, along with affected all-target Clippy and both permission and terminal PTY smokes.
    Static corpus, formatting and dependency-direction checks passed. No CI or live model run was
    performed for this uncommitted branch.

19. **Unified interaction — complete.** INV-3, DRW-3, COM-3 and SEL-5 cover shared focused-menu
    hover/arrow choice, guarded Drawer retraction, conversation-only newline chords and transient
    transport receipts. The user approved the rendered proposal. Actual wide/medium/narrow
    [Drawer controls](../specs/drawer.md#rendered-controls),
    [copy feedback and multiline drafts](../specs/selection-and-copy.md#rendered-feedback), and
    APD approval frames were inspected. Local validation on `daee70c` plus this work passed
    409 TUI, 95 CLI executable, 7 CLI library and 23 measurement tests; the final Drawer grammar
    additionally passed 22 router tests. TUI/CLI all-target Clippy, 37 Python fixture tests,
    formatting, citations, frame references, file length and diff checks passed. Terminal PTY
    verified raw Ctrl-J and Copy sent; permission and status-line PTYs passed with isolated
    loopback fixtures. Terra-max's read-only review has no remaining blockers. CI and live
    desktop clipboard acceptance were not exercised locally.

20. **Drawer handle styling — complete.** DRW-3 uses the user-approved static bottom-center
    `︽` handle and lower outline within the existing border row. Its painted and hit regions share
    one geometry; content height, focus and cancellation behavior are preserved. Local focused
    Drawer and frame tests, TUI all-target Clippy and the revised corner/hover witness passed on
    `1393bcb` plus this change. Six [rendered controls](../specs/drawer.md#rendered-controls) were
    inspected at 120/88/60 columns. Full workspace and terminal verification runs in PR CI;
    no new Kitty windows were opened for this styling change. The corner regression
    witness retains both side glyphs and forbids underlines on them in rest and hover states.

21. **Dependency reuse evaluation — complete.** [The experiment](../spikes/rust-dependency-seeding/README.md)
    did not establish repeatable benefits sufficient to justify a project seeder's maintenance.
    Ordinary Cargo and private task targets remain the build workflow; no seeding command is maintained.

22. **Model selection — complete.** [MDL-1–MDL-4](../specs/model-selection.md) own bounded
    configured `/model` rows, atomic idle replacement, replay compatibility, credential isolation
    and conversation-local overrides. New, resume and restart retain the configured default.
    Local validation passed the 412-test TUI suite, then five focused model tests after refusal
    and direct-click identity fixes; the 104-test CLI suite plus its new reset witness; runtime
    replacement/replay and command environment tests; affected all-target Clippy and corpus gates.
    The two offline PTY journeys prove blank open/filter/Escape, missing-key refusal, a second
    endpoint receiving the retained history/guidance, real command/status credential exclusion,
    and `/new` reset. CI runs the new model journey. Sol-high review found no remaining blockers;
    its direct-click regression failed before the fix and passed afterward.
    Six actual [choice/refusal frames](../specs/model-selection.md#rendered-review) were inspected
    at 120/88/60 columns. Refusal frames use the requested Failure color; only error text styling
    changes, with identical text and geometry. Live-provider acceptance is not claimed.

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
