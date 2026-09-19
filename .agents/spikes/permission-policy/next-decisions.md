# What tool permission is for here, and what was decided

| Field | Value |
| --- | --- |
| Read when | Proposing, judging or rejecting any change to how tool permission works |
| Status | Position decided with the owner 2026-09-19; the confinement it calls for is unbuilt |
| Basis | [Enforcement boundary](./enforcement-boundary.md) for what is true today; [sandbox-boundary](./sandbox-boundary.md) for the mechanism and its measurements; [the spike](./README.md) for the cross-harness comparison |

Read this before writing code or dispatching investigation. It exists because a position agreed in
conversation does not survive a session, and the last attempt spent three agent sweeps re-deriving
what the spike already held.

## Who this is for, and what that removes

Plexmaton has one user: a solo developer, on their own machine, on a project they chose, who picked
the provider and installed every tool that is connected. That is not a detail about the market. It
decides which of the mechanisms in comparable harnesses are load-bearing here and which exist for an
audience we do not have.

**Read other harnesses with the audience separated from the problem.** Claude, Codex and grok all
warn before a capability is used and confine before a command runs, and they must: they cannot know
who is driving or what machine they are on. A prompt that says "this tool needs network access" is
telling its user something they did not already know. Told to this user, about a server they
installed themselves for that purpose, the same sentence is noise. [The spike](./README.md)'s
comparison table is evidence about mechanisms, not a list of features to match.

This harness is **best-effort damage control, not a security boundary**, and says so in its own
documents. It does not model an adversary, and nothing here is built against a malicious model — a
provider is a trust decision the user makes when they configure one, and no local mechanism reaches
upstream of it.

## The rule

> **What we execute ourselves, we confine. What we hand to a subprocess, we do not pretend to.**

One sentence, true of the code today, and it explains both halves of it. The native file tools open
every parent beneath a pinned root with no-follow descriptors, so their authority is genuinely
narrower than the path string a model writes. The shell is handed to `/bin/sh` and the command spec
states plainly that its root is a starting directory rather than a sandbox.

The second half is what confinement changes: it moves the shell from *pretend* to *confine*. Until
it lands, the honest sentence stays.

## What follows

| Decision | Why, and what lost |
| --- | --- |
| **Hands-off inside the blast zone.** The directory the harness was started against, plus scratch and toolchain caches, is allowed without asking | It scores best on every axis below. Rejected: approval prompts for in-zone work, which buy nothing a person can act on and train the reflex that destroys the property they exist for |
| **The boundary controls scope, not consequence.** No undo layer, no snapshot, no softened operations inside the zone | `rm -rf` is irreversible by intent. Rejected: checkpointing the worktree before a turn, which is insurance the system buys against a zone it just called acceptable, and which does not simplify anything — losing uncommitted work already has an answer, and it is `commit` |
| **No command inspection.** No regex, no parser, no dangerous-command classifier | Defeated by `python -c`, `node -e`, `ruby -e`, a written-then-run script, or a build script. An agent that writes code spells an effect any way it likes; matching a command cannot establish its effects, which the spike's own model already states |
| **Network egress is out of scope** | The model API call is already the largest egress channel: every file the agent reads is in the request body before any other destination is reachable. Blocking `curl` guards a door whose room is already empty. Returns only for tools with external side effects — sending mail, opening an issue — where the axis is *irreversible and leaves the machine*, not network in general |
| **No capability warnings** | Telling this user that a server they installed needs the network is telling them what they decided |
| **Show, never ask** | Starting the session is the authorization. A prompt substitutes for the user's decision; a record supports it. What replaces pre-approval is visibility and interruptibility — seeing what is happening, and being able to stop it |
| **Confinement is a recorded fact, not a platform accident** | macOS gets a fence and Linux will not for now. If that difference is implicit, work that succeeds on both succeeds for different reasons and nobody can see which |

## The axes a design is judged on

Fixed with the owner before the decisions above, and they are what those decisions were judged
against. Recorded because a criterion agreed in conversation and not written down is one the next
session re-invents differently.

| Axis | The question | Why it is the one that matters |
| --- | --- | --- |
| Rate | How many approvals per turn and per session | The only number with a denominator |
| Generalisation | Answering once prevents how many future questions | The decisive one: low generalisation compensates with rate, the user starts answering reflexively, and the safety property is gone the moment that begins |
| Answerability | Can a person decide from what is on screen | "Allow Bash?" cannot be answered — Bash does anything |
| Interruption | When the question arrives | A long autonomous run interrupted for something trivial, and a delegated child asking while the user reads the parent, are different costs; the second is ours specifically |

Hands-off scores full marks on all four. That is not an argument against it to be talked around —
it is why the position is what it is, and why the remaining work is a fence rather than a dialogue.

## What is measured, and what that settled

- **Per-invocation cost.** A constant ~6 ms on macOS, under a percent of a command doing real work.
  Cost is not a reason to defer containment on this platform.
- **Cancellation through a confined launch.** `sandbox-exec` applies the profile to itself and execs
  the target, so it adds no process: the tree is structurally unchanged and every cancellation
  invariant holds untouched. 24/24, each arm run bare and wrapped.
- **How a fence fails.** A malformed profile is loud. A well-formed profile with an unresolved
  subpath is silent — accepted, exit 0, command runs, fence grants nothing. Only the second needs
  defending against.

Both probes live beside this file with their results in
[sandbox-boundary](./sandbox-boundary.md).

## What is still open

1. **Where this work belongs.** Phase 04 is product polish; a fence is not. It needs a stage under
   an existing phase or a phase of its own, and that is a roadmap decision.
2. **The write-root list.** Project root, scratch, and the toolchain caches a real build reaches
   (`~/.cargo`, `~/.rustup`, `~/.npm`, Go module cache, uv cache). Reads stay open, which is what
   keeps the profile short and keeps a wrong entry from breaking a build in a way nobody can read.
   This is the part that will need a tail of real-use corrections; its failure mode is a clean
   denied write.
3. **What "show" looks like.** The only additive piece in the whole position, and it is contract
   text: `ui-ux.md` owns it, and it needs a rendered frame the owner has seen.
4. **What the position deletes.** The capability engine that never runs in production, and most of
   PER-1–PER-10 and PGR-1–PGR-5, which describe a mechanism this position does not build. The
   deletion is its own change; a mechanism goes with its spec, tests and citations together.
