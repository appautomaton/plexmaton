# The `.agents` corpus

The project's durable memory. `AGENTS.md` at the repository root is its entry point and the only
part loaded on every turn. Everything here states what is true now.

## Layers

Every byte in `AGENTS.md` is paid on every turn. Every byte below it is paid once, when a task
needs it. A rule earns a place in `AGENTS.md` only if it shapes behaviour before the task is
known; everything else is pushed down until pushing further would hurt.

| Layer | Loaded | Files |
| --- | --- | --- |
| Always | Every turn | `AGENTS.md` |
| On trigger | When the trigger table names it | `standards/*.md` |
| On demand | When the work touches the subject | `roadmap/*`, `specs/*`, `plans/*` |

## Documents

| Document | Holds | Changes when | Ends when |
| --- | --- | --- | --- |
| `AGENTS.md` | How to work here | A rule that shapes every task changes | Never. Growing for a feature means the feature needed a spec |
| `standards/*.md` | How to work in one kind of task | The practice changes | Never |
| `roadmap/plexmaton.md` | Thesis, locked invariants, the phase table, research gates, non-goals | Understanding changes, or a phase opens or closes | Never, and it stays the same size |
| `roadmap/ui-ux.md` | The experience contract, free of mechanism | A rule changes, or a mechanism moves to a spec | Never |
| `roadmap/phase-NN-*.md` | One phase: scope, sequence with current state, exit gate, what is outstanding | Every slice that lands | Closure, below |
| `plans/phase-NN-step-MM-*.md` | One step's slices | Every slice | Consumed. Deleted |
| `specs/*.md` | One mechanism as it is now, with its evidence | The mechanism changes. Rewritten in place; a retired invariant ID is never reused | The mechanism is removed. Spec, tests and citations go in one change |
| `handoffs/*.md` | A letter for whoever picks up the work, written only when the user asks | Never. A stale one is deleted, not corrected | Stale |

Three operations and no others: rewrite in place, delete, append. `AGENTS.md` §Documenting work
owns the first.

## Where we are

Three cells, each saying one thing: the roadmap's active phase, the phase file's status, the
plan's status. Phase, step, slice.

## Specs

A spec defines one mechanism precisely enough to implement and test it: the last thing read before
writing the code, and what a reviewer checks the code against. Write one when a contract is shared
by an implementation and its tests, outlives the current phase, and is too detailed for
`plexmaton.md` yet not a cross-cutting rule for `ui-ux.md`. Where documented types already carry
the contract, the code is the spec.

A spec carries the front matter `Status`, `Owns`, `Depends on`, `Proven by`, and the sections
Purpose, Invariants, Model, Failure modes, Out of scope, Evidence. Invariants are numbered under a
prefix no other spec uses, because they are cited bare, far from the file that defines them. Each
is stated so that a violation is recognisable; one nobody can write a failing test for is prose.
The evidence table maps each invariant to the test that proves it, lists unproven ones as
unproven, and is updated in the same change as the code.

## Plans

A plan turns one delivery step into slices: independently landable pieces, ordered by what
constrains what, each with the test that closes it. Any step big enough to have an order gets one.
It carries `Phase`, `Contract`, `Status` (slice N of M), then Outcome, Slices, Order and why, and
Deliberately not in this plan.

A spec is earned; a plan is cheap. A small mechanism keeps its contract inline in the plan and is
promoted to `specs/` the moment code cites one of its identifiers, because a citation must outlive
the plan. `scripts/check-citations.sh` enforces this.

## Closing

- A **slice** closes with its commit and the test that proves it.
- A **plan** closes by being deleted, with every spec's evidence table current and the phase
  file's sequence saying done.
- A **phase** closes when its exit gate is assessed against the screen and the tests. The roadmap
  row takes the date and one sentence of what was delivered; the next phase's front matter takes
  what it inherits; every spec the phase produced reads Implemented or lists what is unproven;
  contested choices have entries; the file is deleted.
- A **mechanism** is removed by deleting its spec, tests and citations in one change.

Growth goes into specs. A full harness has tens of mechanisms, each with a spec of a few kilobytes
loaded when a task touches it; every other document stays the size it is.

## Adding to the corpus

A new rule must refine an existing rule, in place, or prevent a failure no existing rule prevents,
and name the failure. A new document additionally needs a trigger: a situation that mechanically
implies reading it.

## Budgets

Budgets are in bytes, because a line budget is satisfied by writing longer lines. They warn and
never block: `./scripts/check-doc-budget.sh` reports and exits zero. One firing means content sits
at the wrong layer, so the escape hatch matters more than the number. Short-format documents have
tight budgets because there the number is the mechanism; a phase and the interaction contract are
long-format, and their ceiling only catches runaway growth.

| Path | Budget | Escape hatch when it fires |
| --- | --- | --- |
| `AGENTS.md` | 10 KB | Push the section down to `standards/` and add a trigger row |
| `.agents/README.md` | 8 KB | Split the corpus rules from the budget table |
| `.agents/standards/*.md` | 8 KB | One standard covers one trigger; split by trigger |
| `.agents/specs/*.md` | 12 KB | One spec defines one mechanism; split by mechanism |
| `.agents/plans/*.md` | 8 KB | A plan this long is a phase; the step it plans is too big |
| `.agents/roadmap/plexmaton.md` | 16 KB | Promote detail into a phase file or a spec |
| `.agents/roadmap/ui-ux.md` | 32 KB | Move mechanism detail into `specs/`; keep the rule here |
| `.agents/roadmap/phase-*.md` | 32 KB | Rewrite the scope to what is true; move mechanism detail into `specs/` |
| `.agents/handoffs/*.md` | 8 KB | Delete the stale letter instead of trimming it |
