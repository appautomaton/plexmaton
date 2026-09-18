# Spec — Collaboration turn inclusion

| Field | Value |
| --- | --- |
| Status | Implemented, wired and accepted through delegation, passive restart and actual CLI process-kill recovery |
| Owns | Turn-admission ordering, canonical session references and typed context resolution |
| Depends on | Roadmap §Locked; COL-1–COL-5; JRN-1/JRN-7; TIM-2 |
| Proven by | Ledger/session tests, real-file scripted runtime tests and provider refusal fixtures |

## Invariants

**CIN-1 — Admission freezes the eligible prefix.** A TurnAdmitted item pins recipient, proposed
session head/boundary and turn identity, previous included admission, and all eligible source items
in order up to its own log position. A task update or Handoff accepted before that position
must be included; one accepted later cannot change the frozen turn and remains eligible for a later
admission.

**CIN-2 — Inclusion is a session fact.** One collaboration turn-start atomically records the turn
boundary and canonical admission reference without copying mail/task bodies or creating user input.
The acknowledged session transition also links every newly admitted source reference at that
recipient boundary, so transcript placement remains session-local while the shared body has one
canonical owner.
The previous-inclusion cursor comes from selected session ancestry, so admission without a session
record does not consume mail; missing, foreign and duplicate references fail closed.

**CIN-3 — Resolution preserves attribution and revision.** A resolved context atom contains the
exact immutable admitted source items and their delegation revisions at those source positions, with
bounded transient retention. A provider codec renders those sources in canonical order with each
sender named, or refuses explicitly where it cannot; an unresolved reference is never rendered,
because a pointer would wake the recipient for a message it cannot read. The context budget resolves
the same sources against its own projection: it re-reads the journal rather than the request about
to be sent, so estimating without resolving would measure pointers and refuse a turn the codec
would have encoded.

**CIN-4 — Session acknowledgement precedes driver dispatch.** The collaboration-turn path publishes
no model call before its session inclusion and request authorization appends are acknowledged;
cancelled outer waits retain the owned transition, and uncertain persistence requires reopen.
Replay reconstructs references/context without automatically running providers or tools.

## Model

TurnAdmitted is the logical ordering point in the collaboration log. The later session record
establishes actual inclusion. If the process stops between those writes, a new explicit admission
uses the session's previous included reference and therefore sees the still-pending source items.
Retrying the original item identity resolves that exact frozen admission; it is not permission to
start a second execution. Session boundary validation prevents a stale admission from opening over
an advanced head. The collaboration control owner separately reserves Main authority and binds it
to the exact admission before dispatch; Handoff invalidates unbound tickets. A new explicit
execution needs a fresh turn/admission identity and permit.

The initial eligible set includes mail to the recipient and delegation creation, task updates and
Handoff for either participant. At most 64 source items and 256 KiB semantic
source bytes enter one admission; overflow holds admission rather than truncating task updates.
A transient resolved context cache is capped at 256 admissions and 16 MiB semantic source bytes,
including variable author identities; fixed container overhead is bounded separately by counts. It
is reconstructed from canonical references, never serialized as another task or mail authority.
The runtime path requires an attached journal and explicit driver support. Unsupported drivers or
unavailable historical materialization refuse before a new collaboration turn is written. A later
preparation failure settles the un-dispatched step through the existing turn terminal path.

## Evidence

[Named proofs](../evidence/collaboration-inclusion.md), one row an invariant.

## Integration boundary

Fresh and resumed delegated constructors require an exact durable binding; an internal child
awaiting that binding refuses direct input. Once bound, the runtime gates ordinary direct input
while Main controls and retains a Main permit through inclusion, authorization, owned work and
cancellation/join. [SCH-1–SCH-5](./owned-scheduling.md) supply the asynchronous writer, bounded
runner owner, canonical live-owner wake recheck and resumed selected-branch context restoration.
[CTL-1](./collaboration-tools.md) owns runtime-sealed product ingress authentication. Provider
activation and UI projection are proven by `scripts/smoke-delegate.py`, which delegates through the
real binary against a loopback provider and reads the result in both conversations.
This mechanism does not promise exactly-once model/tool effects or power-loss durability.
Branch/export/delete must retain referenced collaboration logs; unsupported resolution blocks
continuation.
