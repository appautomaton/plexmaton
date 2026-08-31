# The `.agents` corpus

This directory is the project's durable memory. `AGENTS.md` at the repository root is its entry
point and the only part loaded on every turn.

## Why the split exists

Every line in `AGENTS.md` is paid on every turn, for the life of the project. Every line in a
standard or a spec is paid once, when it is relevant. So content is pushed down this hierarchy
until pushing further would hurt: a rule earns a place in `AGENTS.md` only if it shapes behaviour
*before* you know what the task is.

| Layer | File | Loaded |
| --- | --- | --- |
| Always | `AGENTS.md` | Every turn. Identity, always-on invariants, anti-patterns, and the trigger table |
| On trigger | `.agents/standards/*.md` | When the trigger table names it |
| On demand | `.agents/roadmap/*`, `.agents/specs/*`, `.agents/plans/*`, `.agents/DECISIONS.md` | When the work touches that subject |
| On request | `.agents/handoffs/*` | Only when the user asks for one; see its README |

## What goes where

| Document | Answers | Lifetime |
| --- | --- | --- |
| `AGENTS.md` | How to work in this repository | Permanent; must stay short |
| `standards/testing.md` | How to test, and at which tier | Permanent |
| `standards/rust.md` | Code organization, style, and dependency discipline | Permanent |
| `standards/quality-gates.md` | Which gates exist, what each catches, how to run them | Permanent |
| `roadmap/plexmaton.md` | What we are building and why | Permanent; must stay short |
| `roadmap/ui-ux.md` | Cross-phase interaction rules | Permanent |
| `roadmap/phase-NN-*.md` | Scope, evidence, and exit gate for one stretch of work | Time-boxed |
| `specs/*.md` | How one mechanism actually works | Outlives its phase |
| `plans/*.md` | How one step is sliced and sequenced | Deleted when consumed |
| `DECISIONS.md` | When something was decided, what was rejected, what superseded what | Permanent index |
| `handoffs/*.md` | Orientation for whoever picks the work up | Disposable |

## Cite, don't restate

Restating a rule to prove you read it is the largest single source of bloat, and it creates a
second copy that will drift. Reference the identifier instead — `D-027`, `INV-4`,
`phase-00 §delivery sequence`. A citation proves awareness in four words, adds nothing to the
corpus, and stays correct when the source changes.

This applies to conversation, code comments, test names, and commit messages alike. The router
tests are the working example: each names the invariant it defends rather than paraphrasing it.

## Adding to the corpus

A new rule must do one of these, or it does not go in:

- **Refine an existing rule.** Edit that rule; do not add a second one beside it.
- **Prevent a failure no existing rule prevents.** Name the failure.

A new *document* additionally needs a trigger — a situation that mechanically implies reading it.
A document nobody can say when to open is a document nobody opens.

Never let one rule live in two files. Link to the one place that owns it.

## Budgets

Budgets are warnings, not blocks — `./scripts/check-doc-budget.sh` reports and always exits zero.
They exist to catch content sitting in the wrong layer, so the escape hatch matters more than the
number.

| Path | Budget | Escape hatch when it fires |
| --- | --- | --- |
| `AGENTS.md` | 120 | Push the section down to `standards/` and add a trigger row |
| `.agents/README.md` | 100 | Split the corpus rules from the budget table |
| `.agents/standards/*.md` | 150 | One standard covers one trigger; split by trigger |
| `.agents/specs/*.md` | 200 | One spec defines one mechanism; split by mechanism |
| `.agents/plans/*.md` | 150 | A plan this long is a phase; the step it plans is too big |
| `.agents/DECISIONS.md` | 200 | Age the rejected-alternative prose, per below |
| `.agents/roadmap/plexmaton.md` | 250 | Promote detail into a phase file or a spec |
| `.agents/roadmap/ui-ux.md` | 400 | Move mechanism detail into `specs/`; keep the rule here |
| `.agents/roadmap/phase-*.md` | 300 | Age the evidence log, per below |
| `.agents/handoffs/*.md` | 200 | Delete the stale letter instead of trimming it |

## Aging

Documentation earns its length by being read. Three things stop being read long before they stop
being true, so they are compressed on a schedule rather than left to accumulate:

- **Phase evidence.** When a delivery step lands, the previous step's entry compresses to what a
  future reader still needs: corrections to earlier claims, and findings that changed an invariant.
  The narration of how it went goes away.
- **Rejected alternatives.** They exist to stop a settled question being reopened. That pressure
  fades; at a phase's exit gate, the prose collapses into its ledger row and only the verdict
  survives.
- **Plans.** Deleted when their last slice lands. The residue is the commits, the tests, and one
  compressed phase entry — not a checked-off list that becomes a staler second account.
- **Handoffs.** Deleted when stale, never corrected.

Nothing above applies to specs. An invariant with a test behind it is cheap to keep and expensive
to lose.
