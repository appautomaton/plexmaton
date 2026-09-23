# Phase 04 — Product polish and extensibility

| Field | Value |
| --- | --- |
| Status | Active; stages 1–9 and 11–34 complete; stage 10 branding planned and unstarted |
| Parent roadmap | [Plexmaton Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Depends on | Phase 01 interaction ownership; Phase 02 journal/accounting/budget projections; Phase 03 durable collaboration, owned child scheduling, Handoff/Stop, passive recovery and canonical Attention |

## Outcome

A legible default terminal workspace with user-owned presentation customization. Status scripts
consume typed snapshots; the application retains session, input, approval and rendering ownership.
Visual changes are reviewed against real frames before the contract adopts them.

## Scope and sequence

Each finished stage says what is true and what it left open; its spec owns the mechanism and its
evidence file the frames and tests. Git holds how each was validated.

1. **Configurable status line — complete.** [STL-1–STL-4](../specs/status-line.md): a versioned
   snapshot, an owned command, inert styled output and a user-approved variable-height footer.
2. **Session interaction — complete.** JRN-4/JRN-7/JRN-8, SEL-7 and
   [SPK-1–SPK-3](../specs/conversation-picker.md): lazy JSONL creation, restoration feedback,
   message-local retry and edit-retry, hover Copy and the conversation picker. Per-turn usage
   presentation remains a follow-up.
3. **Markdown transcript — complete.** [MD-1–MD-4](../specs/markdown.md): bounded assistant
   formatting that keeps journal source and virtualized scrolling.
4. **Text selection and local approvals — complete.** [SEL-1–SEL-7](../specs/selection-and-copy.md)
   give cross-entry plain-text drags and auto-copy; ATT-1/ATT-3 keep the primary's approvals in its
   conversation, after Esc too, in arrival order. FR-4 records the remaining cold-layout budget gap.
5. **Test-quality hardening — complete.** [Testing](../standards/testing.md) and
   [quality gates](../standards/quality-gates.md) own gate integrity, readiness-driven smokes,
   blocking process fixtures and owned test directories.
6. **Markdown styling — complete.** [MD-5](../specs/markdown.md): the pastel Markdown colours the
   user approved, beside an unchanged status line. A rich-Markdown performance budget remains
   unmeasured.
7. **Responsive native math — complete.** FR-5, MD-4/MD-5, [PRE-1–PRE-4](../specs/render-preparation.md)
   and [MTH-1–MTH-5](../specs/math-layout.md): coalesced frames, owned preparation, native
   conversation math with atomic source copy, capability fallback and clipping; the user approved
   its live appearance and ENT-4's disclosure-only tool click on 2026-09-06. Source reveal, tmux
   sizing, broader font and terminal fidelity, and physical-terminal latency remain follow-ups; this
   is not KaTeX parity.
8. **Agent Skills — complete.** [SKL-1–SKL-6](../specs/agent-skills.md): three skill roots, bounded
   metadata and resource reads, model activation, and durable explicit invocation through edit and
   retry. The [source comparison](../spikes/agent-skills/README.md) is the design evidence; CPL-3
   retains required skills through compaction.
9. **Composer skill picker — complete.** [SKP-1–SKP-4](../specs/composer-menu.md): `$` discovery
   above the primary input with keyboard and pointer completion; literal variables, currency and
   prose stay text.
10. **Branding — unstarted.** The mark, a rounded-square frame around a circular centre, drawn in
    cells at the odd size nearest square for the measured cell, first centred in an empty
    conversation and moving on the one [motion](../specs/motion.md) clock. The
    [stage plan](../plans/phase-04-stage-10-branding.md) owns the order.
11. **Composer menu and Drawer — complete.** A Command is a slash command typed into the
    conversation it addresses ([CMC-1–CMC-3](../specs/composer-menu.md)); the Drawer, pulled by
    `Ctrl-P` ([DRW-1–DRW-4](../specs/drawer.md)), holds what outlives a Session. One
    `PermissionPanel` serves both, and one `Pressed` slot every surface with rows (INV-11). The user
    set the vocabulary and the Drawer's geometry on 2026-09-06. Open: the User rules snapshot, which
    the permission view does not project, and a loopback run of `/compact` through the executable,
    unproven in CMC-1.
12. **Streaming continuity — complete.** MD-4, PRE-3/PRE-4, FR-3/FR-4, MTH-5, ENT-2 and TR-1:
    retained text prefixes, exact painted-source copy, cached refusal identity and bounded capture.
    Release timings and saturated physical-terminal streaming remain unmeasured, and partial-pipe
    fixture readiness is sensitive to concurrent process startup.
13. **Chrome diet — complete.** The composer sits between two rules, grows to a third of the
    terminal and walks a taller draft with `↑`/`↓` and the wheel; a menu is a titled rule closed by
    the composer's top rule. Decided by the user on 2026-09-06; the conversation's own box returned
    once two conversations could share the screen. ENT-1 keeps reasoning source while hiding
    newline-only rows.
14. **Math LaTeX support — complete.** MTH-1–MTH-4 and MD-3 admit, prepare and project the user's
    logits and softmax formulas, accents, Chinese text and boxed mixed-language input, keeping
    unfinished native math compact. Pixel-level streaming flicker remains unverified.
15. **Frozen Markdown prefix — complete.** MD-4 and PRE-1 reuse completed blocks through the owned
    worker, keeping a full parser pass and falling back without losing admissible source. TR-4
    still moves bottom-aligned content as rows grow, so flicker is not shown eliminated.
16. **Reasoning effort — complete.** [EFF-1–EFF-5](../specs/reasoning-effort.md): `/effort` over
    each model's declared levels, the full spectrum with unavailable stops dark and skipped, shared
    composer and selector colours and the bounded max animation; a change applies at an idle
    boundary and the next request carries it. The user tested it in their terminal on 2026-09-23.
17. **Math projection — complete.** MTH-1–MTH-5 and MD-1–MD-4: joined short radicals, the multiline
    log-sum-exp loss and engine-owned compound root indices
    ([review](../evidence/math-layout.md#rendered-and-terminal-evidence)). Tall and script roots
    remain coarse.
18. **Approval inspection — complete.** [APD-1–APD-3](../specs/approval-inspection.md): exact
    command inspection and copy, and return to the pending approval; INV-2/INV-3/INV-11 and
    PER-5/PER-10 guard hover, numbered choices and stale input. The user approved the layout.
19. **Unified interaction — complete.** INV-3, DRW-3, COM-3 and SEL-5: one focused-menu hover and
    arrow choice, guarded Drawer retraction, conversation-only newline chords and transient copy
    receipts, as the user approved ([Drawer](../evidence/drawer.md#rendered-controls),
    [copy and drafts](../evidence/selection-and-copy.md#rendered-feedback)).
20. **Drawer handle styling — complete.** DRW-3's user-approved `︽` handle and lower outline in the
    border row, painted and hit from one geometry
    ([rendered controls](../evidence/drawer.md#rendered-controls)).
21. **Dependency reuse evaluation — complete.** [The experiment](../spikes/rust-dependency-seeding/README.md)
    found no repeatable benefit worth a seeder; ordinary Cargo and private task targets remain.
22. **Model selection — complete.** [MDL-1–MDL-4](../specs/model-selection.md): configured `/model`
    rows, atomic idle replacement, replay compatibility, credential isolation and
    conversation-local overrides; new, resume and restart keep the configured default
    ([review](../evidence/model-selection.md#rendered-review)).
23. **Waiting input — complete.** [IQU-1–IQU-4](../specs/input-queue.md) project the owned queues
    above the composer; `Alt-↑` returns the newest message and its skill only into an empty draft.
    Mid-turn Enter as steering, and cancelling a skill read already in progress, remain separate
    work.
24. **Independent status projections — complete.** STL-3 separates prospective context errors,
    acknowledged accounting and path facts, and status-command execution; resume keeps MDL-4's
    default and MDL-1/BUD-3 retain codec admission. The user confirmed the partial status; the
    [combined frames](../evidence/status-line.md#rendered-projections) await their confirmation.
25. **Transcript group boundaries — complete.** TR-6 makes group spacing one measured rule shared
    by height, scrolling and hover, with the one standard gap the user approved
    ([frames](../evidence/transcript-layout.md#reviewed-frames)).
26. **Conversation tree readability — complete.** TRE-2/TRE-6 derive links, folding and head
    anchors from the retained message tree; the user accepted the native interaction on 2026-09-13
    ([frames](../evidence/conversation-tree.md#native-validation)).
27. **Markdown and syntax theme — complete.** MD-5/MD-6: bounded syntax roles for Rust, Python,
    JSON, JavaScript/TypeScript and Shell in the existing palette, each grammar compiled once per
    process; selection keeps colours and emphasis
    ([frames](../evidence/markdown.md#native-syntax-validation)). The user has not yet reviewed the
    theme in their terminal.
28. **A model switch degrades — complete.** Nothing in a conversation's past refuses a switch
    (MDL-1): replay the destination cannot use is carried as content under PRV-3, a finished thought
    as text kept apart from the answer, an interrupted one omitted, tool-call ids in a shape every
    dialect accepts; switching back replays the originals exactly, and COM-3's route says once what
    a switch cost. The user tested both directions in their terminal on 2026-09-23. Whether live
    providers accept every degraded shape remains unproven, since no local evidence can show it.
29. **Compaction declines instead of paying to find out — complete.** CPL-3 answers before the
    summarizer is asked when a conversation is inside its retention window; `/compact --force`
    removes that gate, CMC-2's first declared flag. Publication no longer refuses a replacement for
    coming back larger, because size is an estimate and quality is unread.
30. **Automatic compaction is visible — complete.** A checkpoint is one system row where it landed,
    mid-turn when the runtime took it on its own, and there again on reopen (CPL-4); the activity
    line says `Compacting…` while any summarizer runs (CPL-6). Requested and automatic compaction
    look the same. The user chose both from [rendered frames](../evidence/compaction.md#rendered).
31. **Test evidence says what kind it is — complete.** A readiness marker is now published by
    rename, which restored real proof for CTL-1, COL-4, COL-5, CHB-2 and CHB-3;
    [testing](../standards/testing.md) places a test by cost and determinism rather than by
    mechanism and states the handshake rule beside its readiness-signal rule.
32. **Inspection stops sharing the control slot — complete.** SCH-2's inspection and control lanes
    are separate: reads wait on their own, while the control slot keeps its refusal so a mutation
    retains its exact attempt.
33. **Provider-side tools — complete.** A model declares the hosted tools its route accepts (PRV-6)
    and a provider-run search is a row in its own colour where the provider began it (PRV-5, ENT-2);
    the harness never runs the search. The [spike](../spikes/provider-side-tools/README.md) keeps
    the one route's live measurements. Where hosted-tool spend belongs in the cost surface stays
    open in [ui-ux](../ui-ux.md) §open questions.
34. **Conversation chrome — complete.** The user's turn sits on a band behind a blue `›`, and the
    composer's rule names the model and its effort (COM-4). The activity line moves on the one
    [motion](../specs/motion.md) clock and reads the turn's time without the user's waits, the
    effort, and the primary's quiet; idle, it gives its rows back (COM-5). Chosen from rendered
    candidates in the [spike](../spikes/conversation-chrome/README.md); the user reviewed the band's
    [frames](../evidence/transcript-entry.md) and the activity line in their own terminal.

Independent lossless session export and import belongs to this phase as unstarted follow-up work:
JRN-3/PRV-3 prove only JSONL save, reopen and exact compatible replay. Rejected: treating JSONL round
trips as export/import acceptance. MCP remains optional later integration. Other roadmap work in
performance, math and extensibility receives a stage when its evidence is ready.

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
