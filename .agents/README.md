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
| On demand | When the work touches the subject | `roadmap.md`, `ui-ux.md`, `phases/*`, `plans/*`, `specs/*`, `research/*`, `spikes/*` |

## Documents

| Document | Holds | Changes when | Ends when |
| --- | --- | --- | --- |
| `AGENTS.md` | How to work here | A rule that shapes every task changes | Never. Growing for a feature means the feature needed a spec |
| `README.md` at the root | What the executable does today, and how to run and gate it | The executable changes | Never |
| `standards/*.md` | How to work in one kind of task | The practice changes | Never |
| `roadmap.md` | Thesis, locked invariants, the phase table, research gates, non-goals | Understanding changes, or a phase opens or closes | Never, and it stays the same size |
| `ui-ux.md` | The experience contract, free of mechanism | A rule changes, or a mechanism moves to a spec | Never |
| `phases/phase-NN-*.md` | One phase: scope, sequence with current state, exit gate, what is outstanding | Every slice that lands | Closure, below |
| `plans/phase-NN-stage-MM-*.md` | One stage's slices | Every slice | Consumed. Deleted |
| `specs/*.md` | One mechanism as it is now, with its evidence | The mechanism changes. Rewritten in place; a retired invariant ID is never reused | The mechanism is removed. Spec, tests and citations go in one change |
| `research/*.md` | A research gate: the invariants its result must satisfy, its corpus, candidates, and decision criteria | The comparison advances | Decided. The result becomes a spec and the file is deleted |
| `spikes/<topic>/README.md` | Bounded investigation: read trigger, question, evidence, run command and limits; prototypes beside it, build output outside the corpus | Evidence changes | Promote decisions; delete when evidence is no longer useful |
| `handoffs/*.md` | A letter for whoever picks up the work, written only when the user asks | Never. A stale one is deleted, not corrected | Stale |

Three operations and no others: rewrite in place, delete, append. `AGENTS.md` §Documenting work
owns the first.

## Where we are

Three cells, each saying one thing: the roadmap's row for the phase, the phase file's status, the
plan's status. Phase, stage, slice.

`phases/` holds every open phase, one file each. Phases are cut by what must be true at their gate,
not by layer, and work is cross-cutting, so more than one can be open; in one worktree one slice is
in progress at a time, and the plan is where that shows. A phase file is created when its work
starts and the evidence to constrain it exists, never earlier to make the roadmap look complete;
until then the phase is a row in the roadmap's table. Phases carry two digits everywhere,
`Phase 00`, so prose, the table and the file names are one greppable identifier.

## Specs

A spec defines one mechanism precisely enough to implement and test it: the last thing read before
writing the code, and what a reviewer checks the code against. Write one when a contract is shared
by an implementation and its tests, outlives the current phase, and is too detailed for
`roadmap.md` yet not a cross-cutting rule for `ui-ux.md`. Where documented types already carry
the contract, the code is the spec.

A spec is the front matter `Status`, `Owns`, `Depends on`, `Proven by`, then Invariants and
Evidence. An invariant is numbered under a prefix no other spec uses, because it is cited bare, far
from the file that defines it, and is stated in one or two sentences a failing test can be written
for; beside it go the cite into the contract it comes from and at most one `Rejected:` sentence.
The evidence table maps each invariant to the test that proves it, lists unproven ones as unproven,
and is updated in the same change as the code. A spec may add a Model, as a diagram, a type, or a
state machine; a Failure modes table; and a table for what the mechanism owns that a test cannot
see, such as a formula or a measured cost. It has no purpose section, because `Owns` is the
purpose; no out-of-scope section, because the phase file routes what is not built; and no ownership
rationale, because the code owns what owns what and the contract owns why. Rejected: a six-section
template with Purpose, Model, Failure modes and Out of scope required, which produced eight-kilobyte
essays restating the contract, and would have made forty specs a second, drifting codebase.

## Plans

A plan turns one delivery stage into slices: independently landable pieces, ordered by what
constrains what, each with the test that closes it. Any stage big enough to have an order gets one.
It carries `Phase`, `Contract`, `Status` (slice N of M), then Outcome, Slices, Order and why, and
Deliberately not in this plan.

A spec is earned; a plan is cheap. A small mechanism keeps its contract inline in the plan and is
promoted to `specs/` the moment code cites one of its identifiers, because a citation must outlive
the plan. `scripts/check-citations.sh` enforces this.

## Closing

- A **slice** closes when implemented and verified. Commits require an explicit user request.
- A **plan** closes by being deleted, with every spec's evidence table current and the phase
  file's sequence saying done.
- A **phase** closes when its exit gate is assessed against the screen and the tests. The roadmap
  row takes the date and one sentence of what was delivered; the next phase's front matter takes
  what it inherits; every spec the phase produced reads Implemented or lists what is unproven;
  each contested choice carries a `Rejected:` sentence beside the rule it explains; the file is
  deleted.
- A **mechanism** is removed by deleting its spec, tests and citations in one change.

Growth goes into specs. A full harness has tens of mechanisms, each with a spec of a few kilobytes
loaded when a task touches it; every other document stays the size it is.

## Adding to the corpus

A new rule must refine an existing rule, in place, or prevent a failure no existing rule prevents,
and name the failure. A new document additionally needs a trigger: a situation that mechanically
implies reading it.

## Budgets

Budgets count bytes, not lines. `./scripts/check-doc-budget.sh` measures tracked and untracked
corpus documents; warnings are advisory. Move excess content to its owning layer using the
escape hatch below.

| Path | Budget | Escape hatch when it fires |
| --- | --- | --- |
| `AGENTS.md` | 10 KB | Push the section down to `standards/` and add a trigger row |
| `README.md` at the root | 4 KB | It says what the executable does and how to run it; anything else belongs in the corpus |
| `.agents/README.md` | 8 KB | Split the corpus rules from the budget table |
| `.agents/standards/*.md` | 8 KB | One standard covers one trigger; split by trigger |
| `.agents/specs/*.md` | 24 KB | Split when a second mechanism or ownership boundary appears; evidence may grow with the contract it proves |
| `.agents/plans/*.md` | 8 KB | A plan this long is a phase; the stage it plans is too big |
| `.agents/roadmap.md` | 8 KB | Detail belongs in a phase file, a research track, or a spec |
| `.agents/ui-ux.md` | 32 KB | Move mechanism detail into `specs/`; keep the rule here |
| `.agents/phases/*.md` | 32 KB | Rewrite the scope to what is true; move mechanism detail into `specs/` |
| `.agents/research/*.md` | 8 KB | A track this long has started building; give it a phase |
| `.agents/spikes/*/*.md` | 8 KB | Promote decisions; keep evidence beside the report |
| `.agents/handoffs/*.md` | 8 KB | Delete the stale letter instead of trimming it |
