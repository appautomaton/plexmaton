# Handoff — the interaction spine, one vertebra in

| Field | Value |
| --- | --- |
| Written | 2026-08-31 |
| Last commit | `feat(tui): add typed intents and the interaction router` |
| Phase | 00 — Experience Skeleton, delivery step 1 of 7 complete |
| Your next move | Delivery step 2 — surfaces with clipping, focus, and modality |

Hello. I built the router you are about to build on top of. This letter is the part that is not in
the repository: what is actually true right now, what only reads as true, and which things already
cost me an afternoon so they do not cost you one.

## Read in this order, and stop when you have enough

1. `AGENTS.md` — the always-on rules and the trigger table that routes you everywhere else.
   `.agents/README.md` explains the layering; `.agents/standards/` holds testing, Rust, and gate
   policy, loaded when the trigger table says so.
2. `.agents/DECISIONS.md` — 34 rows. An index. Read the **Rejected alternatives** section before
   proposing anything that feels obvious; several obvious things were considered and killed with a
   reason you would otherwise rediscover the slow way.
3. `.agents/roadmap/phase-00-experience-skeleton.md` — scope, the 7-step delivery sequence, and the
   dated evidence entries. The evidence entries are the honest record of what runs.
4. `.agents/specs/interaction-routing.md` — INV-1 to INV-9, all proven. This is the contract for
   the code you are about to extend.
5. `.agents/roadmap/ui-ux.md` — only the sections you need. It is long on purpose and it is the
   product contract; you may refine it with evidence, never contradict it silently.

Do **not** pre-load every roadmap phase or all three specs. Progressive disclosure is an enforced
working rule here, not a documentation style.

## How this collaboration runs

Three conventions the user has stated and expects to persist:

- **Reply in Simplified Chinese; write English in code, comments, and documents.** Technical terms
  stay English. Spell out uncommon abbreviations in prose.
- **Organize before you present.** High signal, structured so the user can decide, not a transcript
  of your reasoning. They will read a table; they will not read six paragraphs of narration.
- **Memory discipline.** "The true memory of this project is the harness and the unit tests."
  Nothing gets written down because it might be useful. A document that outruns its evidence is the
  failure mode this project has already hit once, and the correction is in the phase file where
  everyone can see it.

The user reads diagrams and ASCII compositions carefully and catches errors in them. Twice they
caught a mistake I had shipped into a mock. Do not hand-wave a layout you have not actually laid
out.

## What is real, and what merely reads as real

`roadmap/plexmaton.md` describes the intended architecture. Much of it does not exist yet. The gap
is deliberate, but you must not read the roadmap as an inventory.

| Thing | Status in code |
| --- | --- |
| Semantic event boundary (`PrototypeEvent` + envelope) | Real. The only path from the simulator into the projection |
| Revisioned `ViewState`, repaint gating | Real, tested, and load-bearing for the event loop |
| Typed degradation on ordering defects | Real. Gaps, stale sequences, and contract violations degrade visibly and never terminate |
| Semantic colour roles, three palettes | Real. A test asserts all three paint identical characters |
| Five layout classes incl. a too-small notice | Real and tested |
| `TuiIntent` + `Router` | Real, tested, mutation-checked |
| `SurfaceTree` | Exists — insert, z-ordered `hit_test`, `promote`. **No clipping, no focus, no modality.** Empty on the application path |
| Viewports and scroll ownership | Does not exist |
| Transcript virtualization | Does not exist. `render_transcript` builds every line of every item, every frame |
| Composer, inspector, shelf, attention queue | Do not exist |
| Mailbox, delegation record, persistence, providers | Do not exist and are not Phase 00's problem |

Two consequences worth stating plainly:

- **The running binary has no mouse.** Nothing calls `EnableMouseCapture`, and the surface tree the
  router hit-tests against is empty, so pointer events resolve to `Ignored::OutsideWorkspace` in
  the real terminal. Pointer routing is proven by unit tests only. Enabling capture is coupled to
  the terminal-native-selection escape hatch (INV-8), so turn it on deliberately, with the
  Shift-modifier path tested, not as a side effect of step 2.
- **Five intents have no consumer.** `CycleFocus`, `Dismiss`, `Scroll`, `Pointer`, and `Text` are
  produced and tested but nothing acts on them yet. `apply_intent` in `main.rs` lists them
  explicitly instead of using a wildcard, so adding an intent cannot silently do nothing. Keep that
  property. If step 2 stalls, this becomes dead weight — which is the honest argument for doing
  step 2 next rather than something more fun.

## Traps that already cost time

- **A pseudo-terminal with no window size renders nothing.** `script` allocates one that reports
  0×0; Ratatui paints zero cells and any content assertion passes or fails for the wrong reason.
  `scripts/smoke-tui.py` sets `TIOCSWINSZ` explicitly. An earlier evidence claim was wrong because
  of this and had to be retracted in the phase file.
- **No controlling terminal means no `SIGWINCH`.** The child needs `start_new_session=True` plus
  `TIOCSCTTY`, or your resize is silently ignored and the test proves nothing.
- **Ratatui emits only changed cells.** An incremental frame shows `1`, not `attention 1`, and
  unchanged spaces arrive as cursor moves. The smoke forces one full repaint via resize and asserts
  against that frame. Do not assert content against an incremental frame.
- **`q` is the smoke's quit key.** If you change that binding, change `smoke-tui.py` in the same
  commit.
- **The sandbox blocks `pty.openpty`** ("out of pty devices"). Run the smoke outside the sandbox.
- **`missing_docs` workspace-wide produced 61 findings**, nearly all restated signatures, which
  `AGENTS.md` forbids. It is scoped to `plexmaton-core` and should stay there.
- **`cargo deny` rejects our own crates by default** — unlicensed workspace members and path
  dependencies read as wildcards. `deny.toml` handles both. Do not "fix" it by inventing a licence;
  the user has not chosen one.
- **The file-length sentinel measures above the first `#[cfg(test)]`.** Inline tests are free.
  Function-level `too_many_lines` and `cognitive_complexity` are the real guard.

## Rules that will bite you if you skim

- **Every spec invariant names the test that proves it, or is marked unproven.** Update the
  evidence table in the same change that implements the invariant. Two specs
  (`delegation-and-steering`, `mailbox-delivery`) are entirely unproven and say so; that is
  correct, not an omission to tidy up.
- **`ViewState::touch()` only on a user-visible change.** A no-op that advances the revision costs
  a full-screen repaint and breaks `revision_advances_on_visible_change_and_holds_on_a_no_op`.
- **Ordering is arrival order, not identifier order.** `OrderedById` exists for exactly this. A
  mutation check confirmed the two ordering tests catch a regression to `BTreeMap` order.
- **Colour is never the only carrier.** `every_palette_paints_the_same_text` fails if you encode
  meaning in styling alone.
- **Conventional Commits**, and the pre-commit hook runs fmt, the sentinel, clippy, tests, and
  typos. Enable it once per clone: `git config core.hooksPath .githooks`.

Gates are listed in [`standards/quality-gates.md`](../standards/quality-gates.md). 49 tests today.
`./scripts/check-doc-budget.sh` reports and never fails; it is quiet at the moment. When it speaks
up, read the escape hatch for that path in `.agents/README.md` before touching the number.

## Step 2, concretely

Sliced already: [`plans/phase-00-step-02-surfaces.md`](../plans/phase-00-step-02-surfaces.md).
Read that rather than re-deriving it, and delete it when its last slice lands.

The short version — give `SurfaceTree` what the router already assumes it will have, and put it on
the application path. The plan takes the registration seam first, deliberately, so no slice repeats
step 1's tested-but-unconsumed shape. The four questions it answers, in the order they constrain
each other:

1. **Clipping.** A surface has bounds and a clip rectangle; `hit_test` must respect the clip, not
   the bounds. A child clipped by a scrolled parent is the case that breaks naive implementations.
2. **Focus.** Where does the focus ring live, what is in it, and what is the order? `RouterContext`
   currently takes `focus: KeyboardFocus` as a two-state fact supplied by the caller. That was
   right for step 1 and will not survive: it needs to become "which surface holds focus", with
   `KeyboardFocus` derived from that surface's kind.
3. **Modality.** A modal blocks pointer and keyboard delivery below it. This is a property of a
   surface, and it is what turns `RouterContext::dismissible: bool` into a real dismissible stack.
   That `bool` is a deliberate placeholder — it is the shape I would change first.
4. **Registration.** Regions currently come out of `render.rs` as bare `Rect`s computed per frame.
   Layout must produce surfaces, and the renderer must draw the surfaces it registered, or hit
   testing and painting will disagree. Getting this wrong is the classic source of "the click
   landed one panel over".

Then D-026, D-027, and the collapsed composer row become implementable, and step 3 (viewports) has
something to hang scroll state on.

## What I would watch

- `SurfaceTree` is a `BTreeMap` with a linear `hit_test`. Fine for a dozen surfaces; do not
  micro-optimise it, but do not let it become the reason a per-cell hit test is written elsewhere.
- `surface::Point` duplicates Ratatui's `Position`. Harmless, and worth collapsing if you touch
  geometry anyway.
- The five decisions recorded in `ui-ux.md` but unimplemented (shelf geometry, the ten-row
  guarantee, focus-on-open, the collapsed composer row, the reduced drag scope) are the largest
  block of documentation currently ahead of its evidence. Steps 2 and 3 are what pay that down.
  Until then, resist adding a sixth.

Good luck. The invariants are numbered so you can argue with them precisely — if one is wrong, say
which number and why, and change it in the spec before the code.
