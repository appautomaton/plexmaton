# Plans

A plan turns one step of a phase's delivery sequence into **slices**: independently landable pieces
of work, ordered by what constrains what, each with the test that closes it.

It fills the gap between "the contract says what this must do" and "here is a commit". Without it,
a step of any size gets improvised, and the improvisation is only visible afterwards in the diff.

## Lifetime

**A plan is consumed.** When its last slice lands, its residue is the commits, the tests, and one
compressed entry in the phase file. Delete it then — do not leave a checked-off plan lying around
as a second, staler account of what happened.

This is the opposite of a spec. A spec outlives its phase because the code is checked against it
forever; a plan stops being true the moment the work is done.

## A plan versus a spec

| | Spec | Plan |
| --- | --- | --- |
| Answers | How the mechanism must behave | How the work gets cut and sequenced |
| Lifetime | Outlives its phase | Deleted when consumed |
| Earned by | A contract both implementation and tests depend on | Any step big enough to have an order |
| Cost of writing one that was not needed | A permanent document nobody reads | Almost none |

**A spec is earned; a plan is cheap.** Do not split a small mechanism into its own spec file just
because the structure has a slot for it. Carry the contract inline in the plan, and promote it to
`specs/` only when it proves durable — when a second caller depends on it, or when the invariants
outlive the slices that introduced them.

**An identifier the code cites must outlive the code.** This is where the two rules collide: a plan
may carry numbered invariants, and "cite, don't restate" invites a test or a comment to reference
one. Deleting the plan then orphans every citation. Phase 00 did exactly this — nine `COM-N`
citations survived the plan that defined them. So a plan's identifiers stay inside the plan until
the contract is promoted; the moment code cites one, the contract has proven durable and belongs in
`specs/`.

Where a spec already exists, the plan cites it rather than restating it.

## Required shape

```markdown
# Plan — <phase step or change>

| Field | Value |
| --- | --- |
| Phase | which phase and which delivery step |
| Contract | the spec it implements, or "inline below" |
| Status | slice N of M |

## Outcome
What is true when the last slice lands, in two or three sentences.

## Slices
Numbered. Each slice states what it changes, what proves it, and what it unblocks. A slice that
cannot land on its own is not a slice; merge it into its neighbour.

## Order and why
What constrains what. This is the part worth thinking about — a wrong order means building against
a boundary that has not been decided.

## Deliberately not in this plan
What a reader would expect here and will not find, with the plan or phase that owns it.
```

## Naming

`phase-NN-step-MM-<topic>.md`, so the file says where it sits without being opened.
