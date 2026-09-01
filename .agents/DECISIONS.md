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
| D-045 | 2026-08-31 | An action-required event joins a visible, ordered band and takes nothing; going to one is the user's keypress, and acknowledging it is not resolving it | Accepted | [attention](./specs/attention.md) ATT-1 to ATT-3 |
| D-044 | 2026-08-31 | A navigation key means "move inside what holds focus": it chooses an agent only in the rail and scrolls everywhere else, which is how the wheel finally has a keyboard equivalent | Accepted | [interaction-routing](./specs/interaction-routing.md) INV-10 |
| D-043 | 2026-08-31 | A selection is a range over a surface's entries, never over cells; copy returns the producer's source, is bound to `Ctrl-Y`, and is delivered by OSC 52 | Accepted | [selection-and-copy](./specs/selection-and-copy.md) SEL-1 to SEL-5 |
| D-042 | 2026-08-31 | Inspection is an axis of its own: opening does not move the selection, an unpinned inspector follows it, and a pinned one is what puts two agents on screen | Accepted | [inspector](./specs/inspector.md) INS-1 |
| D-041 | 2026-08-31 | The event loop is one `Workspace` the executable and the harness both drive; frame work is asserted and frame time is only reported | Accepted | [frame-loop](./specs/frame-loop.md) FR-1 to FR-3 |
| D-040 | 2026-08-31 | A conversation is measured item by item and cached by revision and width; a reader is parked against a message rather than a row, and each conversation keeps its own | Accepted | [transcript-layout](./specs/transcript-layout.md) TR-1, TR-3, TR-5 |
| D-039 | 2026-08-31 | A viewport measures its content through the same `Paragraph` that paints it, using ratatui's `unstable-rendered-line-info` | Accepted | [surface-model](./specs/surface-model.md) §viewports |
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
| D-013 | 2026-08-31 | Colour is twelve semantic roles over three palettes; `ansi` is the default so the user's terminal theme wins | Accepted | `plexmaton-tui::theme` |
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

### D-043 · Character-granular selection inside a message — rejected

It needs an inverse map from painted cells back through the wrapping cache to byte offsets, and that
is not merely more work: it makes SEL-1 false. A sub-item range is expressed in wrapped rows, so the
same selection would copy different text at a different width — the exact property the entry-based
model exists to guarantee. Nothing in the journey copies half a message.

### D-043 · `arboard` for clipboard access — rejected for now

It reaches the desktop the *process* runs on, which over SSH or inside tmux is the wrong machine —
and the phase file already recorded that a local clipboard cannot be the only path. OSC 52 reaches
the terminal the *user* is at, needs no crate at all (Crossterm has the feature), and covers the
harder case. It has a real cost: the terminal never acknowledges it, and many decline it unless
configured. That cost is stated rather than hidden, which is why the workspace claims nothing about
a copy having landed. A native path waits for a user whose terminal refuses OSC 52.

### D-043 · `Ctrl-C` as the copy binding — rejected

The familiar one, and unavailable: `Ctrl-C` is the unconditional exit (INV-7), and a key that both
copies and ends sessions is worse than an unfamiliar one. `Ctrl-Y` is the binding, and the `Shift`
escape hatch to the terminal's own copy covers the habit.

### D-045 · Resolving an attention item from inside the queue — rejected for now

Approving in place needs a reply channel the Phase 00 runtime does not have, and an
`AttentionResolved` event with no producer would be a mechanism pretending to be a contract. So the
queue has one transition, and it is acknowledgement: a seen request stays queued, because it is
still outstanding.

Only where a serious alternative was considered. The reason matters more than the verdict.

### D-042 · An inspector that replaces the conversation it was opened from — rejected

The simpler model: `Enter` selects and opens, so the inspector shows what the conversation already
shows. Then the shelf displays a second copy of the surface beneath it and "overlay without
occlusion" protects rows nobody needed. Separating the axes is what makes the geometry worth having,
and it is what gives pinning a meaning — a pin is the inspector declining to follow.

### D-042 · Closing an unpinned inspector when the selection moves — rejected

The first implementation, read correctly from `ui-ux.md`'s "pinned is whether a surface survives the
user working elsewhere", and wrong in use: the peek ended at the moment it became useful, which is
when the user returns to the conversation they were reading. An unpinned inspector follows instead.
The contract sentence still holds; it just does not mean the unpinned one dies.

### D-041 · `criterion` as the measurement lane — rejected

Scheduled for this step and did not enter. Its statistics are for a mean, and the interesting
figure here is a tail; more decisively, the load-bearing evidence is work counts — items wrapped,
lines built, entries retained — which a benchmark harness cannot see and an ordinary test asserts.

### D-041 · Asserting wall-clock budgets in the test suite — rejected

Aged: [frame-loop](./specs/frame-loop.md) and [ui-ux](./roadmap/ui-ux.md) §budgets own it, with the
measurement that settled it — the same binary, the same laptop, hours apart, roughly double. A
threshold loose enough not to flake catches nothing; work counts are exact everywhere.

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

Aged: [.agents/README.md](./README.md) owns the trigger layer. A skill is one vendor's mechanism,
and the corpus has to be readable by a person and by any agent, so triggers are prose in a document
rather than a directory only one tool loads.

### D-032 · A blocking document-budget gate — rejected

Aged: [`.agents/README.md`](./README.md) §budgets now owns the reasoning. The verdict is that a
document over budget is a design question, and a blocking gate turns it into pressure to delete a
sentence.

### D-039 · Owning the text wrapping instead — rejected

Aged: the mechanism is in [surface-model](./specs/surface-model.md) §viewports. The objection that
made owning it attractive does not hold: `line_count` runs the same `WordWrapper` the renderer runs,
so it is ratatui measuring its own wrapping rather than a second derivation that could drift. Owning
it meant roughly eighty lines of grapheme-and-width logic reaching the same answer with our own bugs
instead of ratatui's. The exact pin plus a committed lockfile is what makes an unstable API change
surface at a reviewed bump.

### D-040 · A row offset, a per-surface reading position, and bounded overscan — all rejected

Aged: [transcript-layout](./specs/transcript-layout.md) TR-3 and TR-5 own the two positions.
Three verdicts. A **row offset** survives a resize as a number while naming different text, so it
moves the reader while looking like it did not; an item identity is the durable half. Keying a
conversation's position **by surface** breaks step 4 of the journey — leave A, come back, land where
B was. **Bounded overscan** was in the phase's scope and was not built: a synchronous renderer has
no asynchronous fill for it to hide, so it costs wraps and prevents nothing.

### D-038 · Adopting `ratatui-textarea`, and letting Submit write the transcript — both rejected

Aged: [phase-00](./roadmap/phase-00-experience-skeleton.md) §delivery step 3 and
[composer](./specs/composer.md) COM-3 own the reasoning. The crate takes a
`crossterm::event::Event`, and exactly one component here may (D-029). And a submitted message being
a command rather than a write cost real work — sequence numbering had to move into the runtime,
because two sources feeding one monotonic stream cannot both number it — and bought one writer.

### D-037 · Leaving the composer to Phase 01, and folding it into the surfaces step — both rejected

Aged: the outcome is the delivery sequence itself. The verdict is that a phase whose exit gate says
the user can converse cannot defer the one place they type, and that folding it into the surfaces
step would have hidden a whole invariant set — the single cursor, and a submission as a command —
inside a step already about something else.

### D-036 · Numeric identities and a second layout for hit testing — rejected

Aged: [surface-model](./specs/surface-model.md) owns both. A numbered identity is a convention, and
a convention breaks silently when a region is added; a second layout computed for hit testing is the
multiple-sources-of-truth anti-pattern, whose failure here is a click landing one panel over.

### D-035 · `.agents/worktrees/`, and one shared `CARGO_TARGET_DIR` — both rejected

Aged: [standards/quality-gates.md](./standards/quality-gates.md) owns the location and the
fingerprint collision. Two facts it does not record. `.agents/` is tracked, budgeted markdown, and a
directory holding that plus 245 MB of build output per checkout stops being one anyone can describe.
And sharing a target directory — the standard advice, which silently runs the wrong code here —
saves four crate builds out of seventy-six; `sccache` does not reach it either, because absolute
paths enter its cache key.

### D-034 · Folding specs into the plans folder — rejected

Aged: [plans/README.md](./plans/README.md) owns the comparison. The two have opposite lifetimes, so
one folder means keeping dead plans or deleting live contracts.

### D-031 · `Escape` as the quit key — rejected

The prototype shipped this way and it was changed: `Esc` quit while there was nothing to dismiss,
and the moment a shelf or a draft exists the same reflex that closes an overlay ends the session one
press later. [interaction-routing](./specs/interaction-routing.md) INV-6 and INV-7 replaced it.

### D-030 · Putting `TuiIntent` in `plexmaton-core` — rejected

Aged: the crate boundary is in [plexmaton](./roadmap/plexmaton.md). Scroll, focus cycling and
pointer capture are none of a runtime's business; a user action that does need to reach one becomes
a command in core's vocabulary at the composition boundary.

### D-027 · Hiding the unfocused composer entirely — rejected

Aged: [ui-ux](./roadmap/ui-ux.md) §input owns the rule. Hiding recovers three rows instead of one,
which matters at 48 × 12. Worth keeping: performance was never a reason — painting three dim rows
costs nothing a terminal can measure.

### D-028 · Full floating-window drag in Phase 00 — rejected for now

Aged: [ui-ux](./roadmap/ui-ux.md) §drag scope owns it. A shelf is docked by definition, so only its
height is a user choice.

### D-010, D-011 · Workspace-wide `missing_docs`, and a file-length limit as the primary guard — rejected

Aged: [standards/rust.md](./standards/rust.md) and
[standards/quality-gates.md](./standards/quality-gates.md) own the reasoning. The fact that survives
them: `missing_docs` across the workspace produced 61 findings, nearly all restated signatures.

### D-016 supersedes part of the revision-1 floating-window proposal

Revision 1 proposed free two-axis drag and resize for every inspector in Phase 00. The shelf makes
most of that unnecessary: a top-docked panel needs only a vertical resize handle. Free drag is
retained for pinned and maximized surfaces only. This is a deliberate reduction in Phase 00 scope,
not a deferral of a defining behaviour.
