# Spec — Owned collaboration scheduling

| Field | Value |
| --- | --- |
| Status | Implemented, wired and accepted. `scripts/smoke-delegate.py` proves owned Stop beside a responsive root and a Main Handoff followed by focused User child input; tier 2 covers retained ownership and failure paths |
| Owns | Asynchronous collaboration-file ownership, bounded child-runner lanes, scheduling authority and joined stop/Handoff |
| Depends on | COL-3/COL-4; CIN-4; CHB-1–CHB-3; LIVE-1/LIVE-3 |
| Proven by | Runtime tests named below |

## Invariants

**SCH-1 — One asynchronous owner crosses each mutable boundary.** One joined worker exclusively owns
the blocking `CollaborationFile`; one supervised task exclusively owns each `LiveRuntime`. Callers
use bounded typed commands and replies and cannot obtain mutable file/runtime access. Main
scheduling and User input occupy distinct one-slot lanes. Cancelling a reply wait does not cancel
an accepted command or discard its retained result, including regular admission, User input and
Handoff before preflight acknowledgement.

**SCH-2 — Control retains reserved progress.** Normal scheduling, User input, runtime updates,
disposable inspection and control use separate bounded lanes. Saturated normal, User or update
traffic cannot prevent stop or shutdown admission; inspection consumes no control capacity. Stop
admission is synchronous and owner-retained: repeating the same target is idempotent, while a
different target receives one typed in-progress refusal. Accepted User input settles before Stop;
new input cannot enter while Stop is pending, and shutdown retains its exact settlement and draft.
An update names the runner endpoint and process-local generation; output from a retired generation
cannot settle its replacement, including across replacement owner instances. Unexpected runner
termination is one terminal typed owner update emitted after cleanup output; observing a terminal
update also joins that runner, so root activity cannot spin on a finished, unjoined slot.

**SCH-3 — Scheduled execution owns authority exactly once.** The owner checks explicit provider
capability and reserves bounded runner capacity before durable admission. The accepted command carries one resolved admission plus its
non-cloneable reservation and ticket; it binds the permit before child-session inclusion and retains
it through queued/model/tool work, cancellation and join. Generic admission cannot bypass the owned
turn or Handoff paths.

**SCH-4 — Stop and Handoff join all child ownership.** Stop interrupts the addressed runner and
retains its report until accepted work and its permit settle. A schedule already accepted for that
child settles first and returns beside the Stop report; Stop removes that child's queued wake before
control admission, so late activity cannot restart it. Handoff first validates the canonical mutation,
closes normal and regular admission, settles queued and active work, joins any terminal runner whose
observation was cancelled, and only then appends the durable Handoff.
Only then may an owner-issued process-local target activate an exact runner-generation ticket and
admit User input to that canonical live runner; reopen requires explicit cold activation of the
existing delegated journal and rebuilds Handoff-closed Main wake state without dispatch. An exact
durable Handoff retry returns its receipt without stopping User-owned work. Product submission
retains the exact target and input synchronously; activation, Handoff linking and runtime admission
settle through owner activity so journal or provider progress does not hold terminal input.
Stop takes an addressed request that is still waiting for cold activation, returns its exact draft
and starts no runner.
Shutdown resumes internally retained schedule/User-input/Stop/Handoff operations without a caller
token, returns their exact results, joins every runner and then the writer even when quiescence fails.
Dropping a runner aborts its task rather than
detaching it; orderly product teardown uses joined shutdown. Idle, completion, surface closure and
reply cancellation never transfer control.

**SCH-5 — Wake is advisory and canonical.** One content-free hint names an exact live runner
generation and coalesces per child under the runner bound. An idle child supplies a fresh
branch-local boundary and cursor; the owner then rereads canonical eligible facts through CIN-4 and
uses the existing SCH-3 schedule path. Busy and cancelled work retains the exact obligation, while
stale generations, unsupported providers, Stop, Handoff and shutdown refuse or discard it before a
new admission. Registration restores the selected branch's prior resolved collaboration context
before accepting a resumed child. Wake is never restored automatically after process restart.

## Evidence

[Named proofs](../evidence/owned-scheduling.md), one row an invariant.

## Integration boundary

This mechanism ends at exact scheduling requests, typed runtime updates and durable collaboration
facts. PRV-1 owns provider mail representation, and [CMP-1](./collaboration-mail-projection.md)
owns its product projection. Native tools and product composition authenticate the Main ingress
that holds the owner capability. The scheduler adds no Tokio or I/O ownership to `plexmaton-agent`
and does not use provider text as an authority channel.
