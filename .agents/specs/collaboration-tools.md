# Spec — Collaboration tool contract

| Field | Value |
| --- | --- |
| Status | Implemented and wired. Accepted: `scripts/smoke-delegate.py` drives the three Main control tools plus child `send_mail` and proves the task update schedules the exact child once; Main-authored `send_mail` has no PTY evidence |
| Owns | Exact model-visible delegation, mail, task-update and Handoff schemas before authority binding |
| Depends on | COL-1–COL-3; CHB-1; PRV-1; the typed-mail and single-controller rules in the roadmap |
| Proven by | Runtime schema and parsing tests named below |

## Invariants

**CTL-1 — Model arguments carry intent, never authority.** `delegate`, `send_mail`, `update_task`
and `handoff` use closed bounded JSON objects. A Main call may carry only an opaque owner-issued
`target` selector; artifact pointers are opaque sender-scoped selectors. No argument can supply an
author, sender/recipient endpoint, Conversation, canonical delegation, collaboration item, mail
identity, revision or retry identity. Authenticated owner ingress resolves selectors and derives
all canonical facts from its current registry. Its bounded lane retains an accepted command and
owner-generated attempt across caller cancellation and shutdown. Main authorship is issued only by
the user-owned root runtime carrying that exact ingress and a unique runtime-instance token; a raw
endpoint or another runtime built from a cloned catalog cannot bind or project as it. Once
canonical delegation creation is acknowledged, any later provisioning failure returns the exact
opaque target and retains its detailed cause so explicit recovery cannot create a second delegation.

**CTL-2 — Tool visibility follows controller role.** Main has all four provider-neutral
definitions. A delegated child has only `send_mail`, implicitly addressed to its fixed delegator;
it cannot delegate, update a task or hand off control. Main and child mail use distinct trusted
definition identities because their schemas and authenticated endpoints differ. The parser rejects
duplicate artifact selectors before admission. Versioned artifact selectors derive from immutable
sender journal facts projected by the runtime carrying the exact owner authority. A raw journal,
another owner, or a child capability from another canonical provenance cannot register them;
unknown, foreign or ambiguous facts fail atomically before collaboration admission.

## Evidence

[Named proofs](../evidence/collaboration-tools.md), one row an invariant.

## Integration boundary

The role-aware catalog admits only an explicitly bound Main or child capability and rechecks its
definition identity, revision, capability and role before sending a command. The owner derives
mail endpoints and current task/Handoff revisions. Versioned SHA-256 selectors derive from complete
immutable target/artifact origins and expose no canonical IDs. A target registry rebuilds from
canonical delegation creation on resume; artifact registration requires a runtime-sealed immutable
fact from the exact registered runtime instance on the sender's selected journal branch and stages
the complete registry change before mutation.
`delegate` preflights deterministic failures before allocating an identity, then writes canonical
creation before creating the child journal. Post-creation failure returns the exact target; an
explicit resume reuses the canonical worker and journal without another creation or automatic
wake. In-process cancellation is proven to settle that exact attempt once. A real child process
is killed after canonical creation, child-journal creation, runner registration and acknowledged
mail; every cut preserves the opaque target and exact retry receipts, passively starts no work and
explicitly recovers one journal and runner. Production composition installs this ingress
and the child factory; PRV-1 owns attributed collaboration encoding. The executable smoke exercises
creation, mail and Main Handoff through a loopback Chat Completions fixture, followed by focused
User input to the transferred child. It also updates the same target, observes one addressed child
continuation, and verifies the exact durable task record. Four-dialect compatibility evidence
remains separate from that one journey.
