# Decisions

An index, not a fourth copy of the rules. Every decision's detail lives in exactly one place —
`roadmap/plexmaton.md`, `roadmap/ui-ux.md`, or a file in `specs/` — and this file points at it.

What this file holds that no other file can:

- **When** a decision was made, and what drove it.
- **What was rejected**, and why. This is the part that cannot be reconstructed later, and the
  part that stops a settled question from being reopened every few months.
- **What superseded what.** A decision that was narrowed or reversed stays here with its
  replacement named.

Rules for this file: a row is added only when something is actually decided — proposals live in
the active phase document or its design artifact. A row never becomes the only statement of a
rule. Superseded rows are struck through in the Status column, never deleted.

That last rule makes this the one budget with an irreducible half: the ledger grows by a line per
decision forever, and only the prose below it can be aged. It went from 200 lines to 250 on
2026-08-31, after nine of fourteen rejected-alternative blocks had already been compressed. Raise it
again the same way — age first, then move the number.

## Ledger

| ID | Date | Decision | Status | Detail |
| --- | --- | --- | --- | --- |
| D-038 | 2026-08-31 | The composer is first-party; `ratatui-textarea` is not adopted, and a submitted message is a runtime command rather than a write | Accepted | [ui-ux](./roadmap/ui-ux.md) §input, `plexmaton-sim::Runtime` |
| D-037 | 2026-08-31 | The composer is delivery step 3 of Phase 00, so the sequence grows from seven steps to eight | Accepted | [phase-00](./roadmap/phase-00-experience-skeleton.md) §delivery sequence |
| D-036 | 2026-08-31 | Surface identities are named; the renderer returns the registry it drew, and routing hit-tests only that | Accepted | [surface-model](./specs/surface-model.md) SURF-1 |
| D-035 | 2026-08-31 | `.worktrees/` is the single ignored location for parallel checkouts, and each one keeps its own Cargo target directory | Accepted | [standards/quality-gates.md](./standards/quality-gates.md) |
| D-034 | 2026-08-31 | A plan slices one delivery step and is deleted when consumed; a spec is earned, a plan is cheap | Accepted | [plans/README.md](./plans/README.md) |
| D-033 | 2026-08-31 | Cite identifiers rather than restating rules, in conversation, comments, tests, and commits | Accepted | [.agents/README.md](./README.md) |
| D-032 | 2026-08-31 | Documents are layered by load-time; only `AGENTS.md` is always-on, and every layer has a warn-only budget | Accepted | [.agents/README.md](./README.md), `scripts/check-doc-budget.sh` |
| D-031 | 2026-08-31 | `Escape` resolves one interaction layer per press and never quits; `Ctrl-C` always quits and `q` quits only from a navigation surface | Accepted | [interaction-routing](./specs/interaction-routing.md) INV-6, INV-7 |
| D-030 | 2026-08-31 | The intent vocabulary lives in `plexmaton-tui`; `plexmaton-core` stays the runtime-to-projection semantic boundary | Accepted | `plexmaton-tui::intent` |
| D-029 | 2026-08-31 | One router owns terminal-event translation, and declining an event is a named outcome rather than a fallthrough | Accepted | [interaction-routing](./specs/interaction-routing.md) INV-1 |
| D-028 | 2026-08-31 | Direct manipulation in the first slice is shelf vertical resize only; free panel movement is deferred | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-027 | 2026-08-31 | While a sub-agent's input is active the primary composer collapses to one row rather than hiding | Accepted | [ui-ux](./roadmap/ui-ux.md) §input |
| D-026 | 2026-08-31 | Opening a sub-agent focuses it, so its input is usable immediately | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-025 | 2026-08-31 | Minimum terminal is 48 × 12; below it one explicit notice and no workspace content | Accepted | `LayoutClass::for_size` |
| D-024 | 2026-08-31 | Ultrawide starts at 132 and holds exactly one secondary column, replaced on selection | Accepted | `LayoutClass::for_size`, [ui-ux](./roadmap/ui-ux.md) |
| D-023 | 2026-08-31 | A shelf guarantees ten readable rows of the primary conversation | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-022 | 2026-08-31 | A sub-agent's input takes rows from its own budget, never from the primary conversation's guarantee | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-021 | 2026-08-31 | `specs/` holds mechanism definitions; every invariant names the test that proves it | Accepted | [specs/README.md](./specs/README.md) |
| D-020 | 2026-08-31 | Inbox and Attention queue are projections over one item log, never separate stores | Accepted | [mailbox-delivery](./specs/mailbox-delivery.md) INV-1, INV-7 |
| D-019 | 2026-08-31 | A delegation is one authoritative record with two writers; the user's steer is an amendment, not a bypass | Accepted | [delegation-and-steering](./specs/delegation-and-steering.md) |
| D-018 | 2026-08-31 | Exactly one cursor exists on screen; a sub-agent's input renders only while its surface has focus | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-017 | 2026-08-31 | The composer is bound to the primary agent and is never retargeted by selection | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-016 | 2026-08-31 | A peek renders as a shelf docked to the top of the conversation, guaranteeing ten readable rows below | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-015 | 2026-08-31 | Four layout classes; at ultrawide two conversations sit side by side | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-014 | 2026-08-31 | The agent column sits on the left and carries agents, attention, and activity as three surfaces in one box | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-013 | 2026-08-31 | Colour is eleven semantic roles over three palettes; `ansi` is the default so the user's terminal theme wins | Accepted | `plexmaton-tui::theme` |
| D-012 | 2026-08-31 | Commit messages follow Conventional Commits | Accepted | [AGENTS.md](../AGENTS.md) |
| D-011 | 2026-08-31 | Sprawl guards are function-level first; a 400-line file sentinel excludes inline tests | Accepted | [AGENTS.md](../AGENTS.md), `clippy.toml` |
| D-010 | 2026-08-31 | `missing_docs` is denied in `plexmaton-core` only | Accepted | `crates/plexmaton-core/src/lib.rs` |
| D-009 | 2026-08-31 | The licence allow-list is exactly the licences in the resolved graph; workspace members are skipped rather than given a placeholder | Accepted | `deny.toml` |
| D-008 | 2026-08-31 | Duplicate Ratatui or Crossterm generations fail the build | Accepted | `deny.toml` |
| D-007 | 2026-08-31 | Math rendering is a research track, not part of Phase 00 | Accepted | [track-math-rendering](./roadmap/track-math-rendering.md) |
| D-006 | 2026-08-31 | An exhausted child viewport does not propagate scroll to its parent | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-005 | 2026-08-31 | Plexmaton owns the full alternate screen; it does not render into inline scrollback | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-004 | 2026-08-31 | `ViewState` is revisioned and the executable repaints only when the revision advances | Accepted | `plexmaton-tui::state` |
| D-003 | 2026-08-31 | Ordering defects degrade visibly instead of terminating the workspace | Accepted | `plexmaton-tui::state::ViewState::apply` |
| D-002 | 2026-08-30 | Four crates; `plexmaton-core` depends on no terminal, async, or rendering crate | Accepted | [plexmaton](./roadmap/plexmaton.md) |
| D-001 | 2026-08-30 | Rust with Ratatui and Crossterm; unidirectional state flow with rendering as pure projection | Accepted | [plexmaton](./roadmap/plexmaton.md) |

## Rejected alternatives

Only where a serious alternative was considered. The reason matters more than the verdict.

### D-017 · One composer that retargets on selection — rejected

Aged: [ui-ux](./roadmap/ui-ux.md) §input owns the rationale. The verdict is that a target which
follows the selection is invisible state, and a misdirected steer to a running worker is not undone
by sending another one.

### D-019 · Forbidding direct user steering — rejected

Routing every instruction through the delegating agent removes divergence by construction, but it
is a game of telephone, and it contradicts a locked product goal: the user steers, pauses, and
aborts workers through explicit actions.

### D-019 · Allowing steering that the delegator never sees — rejected

The cheapest option and the worst. The delegator's model of the task goes stale invisibly, which
is the "multiple sources of truth with synchronisation code between them" anti-pattern in its
textbook form. It also produces a failure the user cannot diagnose: the delegator reports one
thing while the worker does another.

### D-032 · Claude Code skills as the trigger layer — rejected

Skills are natively the shape this needed: a one-line description always in context, a body loaded
on invocation. They were rejected because Plexmaton is itself an agentic harness, and locking its
engineering standards into one vendor's format contradicts the product. Portable markdown plus a
trigger table works for any agent. A skill may still be added later as an accelerator whose body
does nothing but point at the file that owns the content.

### D-032 · A blocking document-budget gate — rejected

Aged: [`.agents/README.md`](./README.md) §budgets now owns the reasoning. The verdict is that a
document over budget is a design question, and a blocking gate turns it into pressure to delete a
sentence.

### D-038 · Adopting `ratatui-textarea`, and letting Submit write the transcript — both rejected

The crate was in the candidate table for a composer spike, and its own API decided the question:
it takes a `crossterm::event::Event`. Feeding it one would put a second terminal-event consumer in
a workspace where exactly one component may have that (D-029, INV-1), and driving it through its
lower-level editing calls instead would leave us using a few per cent of it for a model that is
four verbs — insert, delete, newline, submit — with no cursor movement in the key grammar to need
anything more. `unicode-segmentation` and `unicode-width` were already audited into the foundation
and are what the four verbs actually require.

Letting `Submit` append to the transcript directly was the shorter path and would have made the
projection a second writer over state the event stream owns — the divergence this project already
rejected for delegation records. A submitted message is a command; it reaches the screen as the
runtime's own events or not at all. The cost is real and was accepted: sequence numbering had to
move out of the scenario fixture into the runtime, because two sources feeding one monotonic stream
cannot both be numbering it.

### D-037 · Leaving the composer to Phase 01, and folding it into the surfaces step — both rejected

Deferring it was rejected on the exit gate, not on taste. Three of its criteria need a real text
input: continuing to interact with A while B streams, performing the canonical journey by keyboard
alone, and copying from semantic source. INV-2 also stays proven only against fixtures while
`KeyboardFocus::TextInput` is unreachable in the running binary, which is precisely the "documented
behaviour with no implementation behind it" this phase already had to correct once.

Folding it into step 2 was the tempting option, since focus is where the cursor question lives. It
was rejected because that step would then own five slices of geometry plus an editing model, and a
step whose plan needs a phase-sized document is a step that was not cut. Its own step also gives the
`ratatui-textarea` question a place to be answered with evidence rather than by default.

### D-036 · Numeric identities and a second layout for hit testing — rejected

Aged: [surface-model](./specs/surface-model.md) owns both mechanisms. The verdicts are that a
numbered identity is a convention, and a convention is exactly what breaks silently when a region is
added; and that a second layout computed for hit testing is the multiple-sources-of-truth
anti-pattern, whose failure mode here is the click that lands one panel over — visible only to
whoever is clicking.

### D-035 · Worktrees under `.agents/worktrees/` — rejected

Mechanically it works — a nested worktree there is invisible to both corpus scripts — and it was
rejected on meaning. `.agents/` is the project's declared durable memory of tracked, budgeted
markdown; a worktree is untracked, disposable, and 245 MB of build output each, and a directory
holding both stops being one anyone can describe. `.worktrees/` at the root carries the same vendor
neutrality, which was the real point. A second ignored path for whatever a harness defaults to was
rejected with it: two locations is not a convention.

### D-035 · One `CARGO_TARGET_DIR` shared across worktrees — rejected

The standard advice, and it silently runs the wrong code here; the fingerprint collision and its
one-minute reproduction are owned by [standards/quality-gates.md](./standards/quality-gates.md).
What that file does not record: sharing saves four crate builds out of seventy-six, and `sccache`
was considered as the cross-checkout answer and does not reach it either — absolute paths enter its
cache key, and the `SCCACHE_BASEDIRS` escape hatch needs statically configured directories, which is
the opposite of a worktree per task.

### D-034 · Folding specs into the plans folder — rejected

Aged: [plans/README.md](./plans/README.md) now owns the comparison. The verdict is that the two have
opposite lifetimes, so one folder would mean either keeping dead plans or deleting live contracts.
The useful half was kept — a spec is earned, not written by default.

### D-031 · `Escape` as the quit key — rejected

The fact worth keeping is that the prototype shipped this way and it was changed: `Esc` quit while
there was nothing to dismiss, and the moment a shelf or a draft exists the same reflex that closes
an overlay would end the session one press later. The rules that replaced it are
[interaction-routing](./specs/interaction-routing.md) INV-6 and INV-7.

### D-030 · Putting `TuiIntent` in `plexmaton-core` — rejected

Aged: the crate boundary is stated in [plexmaton](./roadmap/plexmaton.md) and the module's own
documentation. The verdict is that scroll, focus cycling and pointer capture are none of a runtime's
business, and a user action that does need to reach one becomes a command in core's vocabulary at
the composition boundary rather than widening the intent enum across both worlds.

### D-027 · Hiding the unfocused composer entirely — rejected

Hiding recovers three rows instead of one, which matters at 48 × 12 where the composer is a quarter
of the screen. Aged: [ui-ux](./roadmap/ui-ux.md) §input owns the rule and its reasoning — a vanished
composer jumps the tail the user is reading, and removes the evidence that the primary agent is
still addressable. Worth keeping here: performance was not a reason. Painting three dim rows costs
nothing a terminal can measure.

### D-028 · Full floating-window drag in Phase 00 — rejected for now

Aged: [ui-ux](./roadmap/ui-ux.md) §drag scope owns it. The verdict is that a shelf is docked by
definition, so only its height is a user choice, and nothing in the canonical journey yet needs a
panel moved to an arbitrary corner.

### D-010, D-011 · Workspace-wide `missing_docs`, and a file-length limit as the primary guard — rejected

Aged: the reasoning is now owned by [standards/rust.md](./standards/rust.md) and
[standards/quality-gates.md](./standards/quality-gates.md), so only the fact that cannot be
reconstructed from them survives. Enabling `missing_docs` across the workspace produced 61
findings, nearly all restated signatures — the filler `AGENTS.md` forbids.

## Superseded

### D-016 supersedes part of the revision-1 floating-window proposal

Revision 1 proposed free two-axis drag and resize for every inspector in Phase 00. The shelf makes
most of that unnecessary: a top-docked panel needs only a vertical resize handle. Free drag is
retained for pinned and maximized surfaces only. This is a deliberate reduction in Phase 00 scope,
not a deferral of a defining behaviour.
