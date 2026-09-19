# What a permission design has to satisfy, and what is still open

| Field | Value |
| --- | --- |
| Read when | Proposing, judging or rejecting any change to how tool permission works |
| Status | Draft. Criteria agreed with the owner; no fork picked |
| Basis | [Enforcement boundary](./enforcement-boundary.md) for what is true today; [the spike](./README.md) for the comparison |

Read this before writing code or dispatching investigation. It exists because criteria agreed in
conversation do not survive a session, and the last attempt spent three agent sweeps re-deriving
what [the spike](./README.md) and [sandbox-boundary](./sandbox-boundary.md) already held.

## What "non-intrusive" has to mean before anything is built

The spike's question asks for fewer approvals. That phrasing hides the failure it is trying to
avoid, so the owner and this agent fixed four axes that a design can actually be judged on. Recorded
here because a criterion agreed in conversation and not written down is a criterion the next session
re-invents differently.

| Axis | The question | Why it is the one that matters |
| --- | --- | --- |
| Rate | How many approvals per turn and per session | The only number with a denominator, and there is none today |
| Generalisation | Answering once prevents how many future questions | The decisive one: a design with low generalisation compensates with rate, the user starts answering reflexively, and the safety property is gone the moment that begins |
| Answerability | Can a person decide from what is on screen | "Allow Bash?" cannot be answered — Bash does anything. "Allow writing outside the repository?" can |
| Interruption | When the question arrives | A long autonomous run interrupted for something trivial, and a delegated child asking while the user is reading the parent, are two different costs; the second is ours specifically, because we delegate |

Elegance, in this repository's sense, is one rule stated once. PER-1–PER-10 plus PGR-1–PGR-5 is
fifteen invariants for one mechanism; the count alone is a question worth asking of whatever
replaces it.

## The hypothesis on the table, and what would kill it

> Make the boundary the default answer, and ask only when a call crosses it.

Three consequences, one per axis: a call inside the boundary runs silently (rate); the question
names the crossing rather than the tool (answerability); the answer attaches to the crossing rather
than the call, so answering once covers every future call that crosses the same way
(generalisation).

This is the shape the three containment-bearing harnesses already implement, under their own names,
per the table above. It is not yet this harness's shape, and two things could prevent it being:

- ~~Cost.~~ Measured, and it does not kill the hypothesis: a constant ~6 ms per invocation on
  macOS, shrinking to under a percent for any command doing real work
  ([sandbox-boundary](./sandbox-boundary.md)). Linux and Windows are still unmeasured, so this
  removes the objection on one platform, not on all three.
- **Reach.** The file tools are already confined and the shell is not. A boundary that covers only
  the tools that were never the risk buys no silence at all.

## The forks a later session should pick from, not re-derive

1. **Is the child floor keyed to effect or to name?** Today, name. Making it effect-keyed means the
   permission engine gains a delegation dimension and `NativeToolProfile` disappears; leaving it
   means CHB-1 gains a sentence admitting the mechanism, and the governing sentence in `tools.rs`
   gains an exception. One of those has to happen; the current state is that both documents are
   silent and one of them is false.
2. **Does containment come before or after the rule work?** [The spike](./README.md) chose rules
   first and containment later, before the compositional rule above was visible. That ordering is
   worth re-testing now, because containment is what makes silence safe, and rules without it only
   move questions around.
3. **Does a persistent Deny enter the interface?** Cheap, and it closes the one gap where the most
   powerful rule shape is unreachable from the product.
4. **Do fifteen invariants survive?** Or does this mechanism get one rule and a boundary.

## Deliberately not decided here

All four forks. This file records what is true, what the criteria are, and what is missing, so the
next session starts from a position rather than from three agents re-reading the same corpus.
