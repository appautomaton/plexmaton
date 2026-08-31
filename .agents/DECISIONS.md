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

## Ledger

| ID | Date | Decision | Status | Detail |
| --- | --- | --- | --- | --- |
| D-031 | 2026-08-31 | `Escape` resolves one interaction layer per press and never quits; `Ctrl-C` always quits and `q` quits only from a navigation surface | Accepted | [interaction-routing](./specs/interaction-routing.md) INV-6, INV-7 |
| D-030 | 2026-08-31 | The intent vocabulary lives in `plexmaton-tui`; `plexmaton-core` stays the runtime-to-projection semantic boundary | Accepted | `plexmaton-tui::intent` |
| D-029 | 2026-08-31 | One router owns terminal-event translation, and declining an event is a named outcome rather than a fallthrough | Accepted | [interaction-routing](./specs/interaction-routing.md) INV-1 |
| D-028 | 2026-08-31 | Direct manipulation in the first slice is shelf vertical resize only; free panel movement is deferred | Accepted | [ui-ux](./roadmap/ui-ux.md) |
| D-027 | 2026-08-31 | An unfocused primary composer collapses to one row rather than hiding or staying full height | Accepted | [ui-ux](./roadmap/ui-ux.md) |
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

A single input whose target follows the selected agent is fewer moving parts, and it is a mode
error generator. The target is invisible state: you glance away, the selection changes, you type,
and the message reaches the wrong worker. There is no visual cue reliable enough to prevent it,
and a misdirected steer to a running agent is not undone by sending another one.

Two inputs, each living inside the surface it belongs to, make the answer to "where does this
keystroke go" a physical fact rather than something to remember.

### D-019 · Forbidding direct user steering — rejected

Routing every instruction through the delegating agent removes divergence by construction, but it
is a game of telephone, and it contradicts a locked product goal: the user steers, pauses, and
aborts workers through explicit actions.

### D-019 · Allowing steering that the delegator never sees — rejected

The cheapest option and the worst. The delegator's model of the task goes stale invisibly, which
is the "multiple sources of truth with synchronisation code between them" anti-pattern in its
textbook form. It also produces a failure the user cannot diagnose: the delegator reports one
thing while the worker does another.

### D-031 · `Escape` as the quit key — rejected

The prototype shipped with `Esc` quitting, which was convenient while there was nothing to dismiss.
It cannot survive the interaction spine: `Escape` is the key people press to back out of a mistake,
so the moment a shelf, a menu, or a draft exists, the same reflex that closes an overlay would end
the session one press later. Reserving it for the ladder costs one binding and removes a whole
class of destructive misfire. `Ctrl-C` is the unconditional exit; `q` is the convenient one, and it
is unreachable while a text input holds the cursor because there `q` is a letter.

### D-030 · Putting `TuiIntent` in `plexmaton-core` — rejected

The Phase 00 crate sketch originally listed intents under core. Core is what a future real runtime
and the projection agree on; scroll, focus cycling, and pointer capture are none of the runtime's
business, and putting them there would make the semantic boundary a grab bag. When a user action
does need to reach the runtime it becomes a command in core's own vocabulary, converted at the
composition boundary — not by widening the intent enum until it spans both worlds.

### D-027 · Hiding the unfocused composer entirely — rejected

Hiding recovers three rows instead of one, which matters at 48 × 12 where the composer is a quarter
of the screen. It was rejected for two reasons, and performance was not one of them: painting three
dim rows costs nothing a terminal can measure.

A vanished composer moves the transcript's bottom edge, and because the transcript follows its
tail, the line the user is actually reading jumps three rows on every focus change. It also removes
the affordance — someone working inside a sub-agent has no visible evidence they can still address
the primary one. Collapsing to a single row keeps both the evidence and two of the three rows.

### D-028 · Full floating-window drag in Phase 00 — rejected for now

Free two-axis movement with eight-way resize needs pointer capture on both axes, boundary clamping,
resize recovery, and keyboard equivalents for every gesture — roughly three to four times the work
of a vertical resize handle. A shelf is docked by definition, so its position is not a user choice;
only its height is. Nothing in the canonical journey needs a panel moved to an arbitrary corner
yet, so the gesture waits until a pinned or maximized surface gives it a reason.

### D-010 · `missing_docs` across the whole workspace — rejected

Enabling it workspace-wide produced 61 findings, nearly all on view-struct fields whose contract is
already obvious. `AGENTS.md` asks documentation to carry invariants rather than restate signatures,
so a workspace-wide deny would have pushed exactly the filler it forbids. Scoping it to the
semantic contract crate keeps the enforcement where field invariants genuinely exist.

### D-011 · A hard file-length limit as the primary guard — rejected

`AGENTS.md` says to split by responsibility and invariant, "not by arbitrary line counts", and
Rust has no file-length convention to borrow. Function-level thresholds are the real guard, because
a large file of small functions is usually fine while a long function never is. The file sentinel
is a secondary signal for a module that accumulated responsibilities without any single function
growing.

## Superseded

### D-016 supersedes part of the revision-1 floating-window proposal

Revision 1 proposed free two-axis drag and resize for every inspector in Phase 00. The shelf makes
most of that unnecessary: a top-docked panel needs only a vertical resize handle. Free drag is
retained for pinned and maximized surfaces only. This is a deliberate reduction in Phase 00 scope,
not a deferral of a defining behaviour.
