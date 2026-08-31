# Specs

A spec defines **one mechanism** precisely enough to implement and test it. It is the last thing
you read before writing the code, and the thing a reviewer checks the code against.

## What belongs here, and what does not

| Document | Answers | Lifetime |
| --- | --- | --- |
| [`roadmap/plexmaton.md`](../roadmap/plexmaton.md) | What we are building and why | Permanent; must stay short |
| [`roadmap/ui-ux.md`](../roadmap/ui-ux.md) | Cross-phase interaction rules | Permanent |
| `roadmap/phase-NN-*.md` | Scope, evidence, and exit gate for one stretch of work | Time-boxed |
| **`specs/*.md`** | **How one mechanism actually works** | **Outlives the phase that introduced it** |

Write a spec when all three are true:

1. The mechanism has a contract that both an implementation and its tests depend on.
2. It is durable beyond the current phase.
3. It is too detailed for `plexmaton.md` and not a cross-cutting interaction rule, which would
   belong in `ui-ux.md`.

Do **not** write a spec to make the folder look complete. Expand one just in time, the same rule
the roadmap applies to phases. A mechanism whose design is still an open question stays an open
question in the roadmap until it is decided.

Do not restate a rule that already lives in `ui-ux.md` or `plexmaton.md`. Link to it. The spec
adds the mechanism — the state machine, the arithmetic, the failure modes — not a second copy of
the principle.

Where the code already carries the contract in documented types, prefer the code. The semantic
event vocabulary in `plexmaton-core` is its own spec; duplicating it here would create exactly the
drift these files exist to prevent.

## Required shape

Every spec carries this front matter and these sections.

```markdown
# Spec — <mechanism>

| Field | Value |
| --- | --- |
| Status | Draft / Accepted / Implemented |
| Owns | the single mechanism this file defines |
| Depends on | other specs or locked invariants |
| Proven by | tests, or "not implemented yet" |

## Purpose
Two or three sentences: what it is, and what breaks without it.

## Invariants
Numbered `INV-1`, `INV-2`, … Each one must be independently testable and stated so that a
violation is recognisable. An invariant nobody can write a failing test for is prose, not a spec.

## Model
Types, state machine, ownership.

## Failure modes
Every way it can go wrong, and the defined response. Silence is never a response.

## Out of scope
What this spec deliberately does not decide, and which document decides it.

## Evidence
A table mapping each invariant to the test that proves it. Unproven invariants are listed as
unproven rather than omitted.
```

## The evidence rule

The evidence table is the point of this folder. An invariant with no test is an assertion, and
this project treats the harness and its tests as the authoritative memory — a document that
outruns its evidence is the failure mode we have already hit once.

When you implement part of a spec, update its evidence table in the same change. When you find a
spec claim that no test supports, mark it unproven rather than leaving it to read as fact.
