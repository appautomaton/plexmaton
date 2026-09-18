# Spec — Collaboration ledger

| Field | Value |
| --- | --- |
| Status | Implemented, wired and accepted. `scripts/smoke-delegate.py` proves durable delegation/mail/Handoff, owned child Stop and post-transfer User input; tier-2 tests retain the authority and failure boundaries |
| Owns | Canonical collaboration admission, retry identity, bounded reduction and delegation task/control authority |
| Depends on | Roadmap §Locked; JRN-4/JRN-7 for the process-death durability boundary |
| Proven by | Pure ledger, real-file authority and permit-backed runtime integration tests |

## Invariants

**COL-1 — One log, one ordered admission.** One stable collaboration identity owns contiguous,
one-based items, each binding one exact event; derived mail and delegation views rebuild from them.
Exact item retries return the original receipt before revision/capacity checks, while conflicting
item identities, sender-scoped mail identities and producer-scoped Attention identities reused
under another item are refused unchanged. Attention resolution requires its exact earlier request
reference and cannot repeat under another item.

**COL-2 — Admission is bounded.** Constructors, admission and replay enforce the bounds below,
including distinct artifact pointers and a mail-inaccessible control reserve.
Capacity refusal is explicit and changes nothing; accepted facts are never silently evicted.

**COL-3 — A delegated Conversation has one controller.** Creation fixes delegator and worker,
refuses cycles/reused workers and starts Main control. Only the fixed delegator may update the task
or complete Handoff at the exact current delegation revision; each mutation advances it,
Handoff is one-way, and later Main mutations are refused. A resolved admission is inspectable data,
not execution authority: the storage owner issues one non-cloneable permit under current Main
control, and Handoff requires every reservation or permit to be disposed. A permit is bound to one
physical authority and exact admission, can be issued only once, and retains writer authority until
it drops. After acknowledged Handoff, direct input requires a process-local owner-issued target for
the exact canonical worker; activation issues an exact runner-generation ticket and the runtime
rechecks current User control. A target or ticket from another owner or runner generation fails
before input; an exact durable Handoff retry returns its receipt
without interrupting User-owned work. Unknown Handoff writes freeze both Main and User authority
until reopen reconciles the canonical prefix. Slice 3 owns Main execution retention; SCH-2/SCH-4
own the bounded User input lane and its retained settlement.
Rejected: concurrent Main/user writers with precedence and objections, which required conflict
arbitration when an explicit handoff gives each input one owner.

**COL-4 — Acknowledgement follows the file append.** The exclusive writer validates and encodes,
then reduces the exact item only after successful unbuffered append and returns its receipt.
Write failure yields an unknown outcome and poisons the writer until reopen; even an old exact
retry cannot report success through that poisoned writer.

**COL-5 — Reopen validates a bounded prefix.** A distinct format/schema identifies the collaboration;
complete records undergo the same reduction checks as live admission before any tail repair.
Recovery follows the table below, writer/tail files are owner-only, and concurrent writers are
refused.

## Model

| Retained resource | Policy |
| --- | --- |
| Summary or task | Nonempty; a hard UTF-8 ceiling each |
| Identity | Nonempty; a hard ceiling |
| Artifact references per mail | Distinct conversation/artifact pairs, hard-capped |
| Total items | One ceiling over mail, task, Handoff, Attention-reference and turn records; configurable downward |
| Delegations | Configurable downward |
| Semantic mail bytes | Text plus endpoint/pointer identities; configurable downward |
| Control reserve | Tail item slots mail cannot take; configurable from zero to total items |

Values: `plexmaton-agent/src/collaboration/types.rs`.

Mail addresses peers declared through delegation creation; declaration proves neither a session
file nor artifact availability. Item count also bounds retained control text and deduplication
indexes. The control reserve does not promise indefinite admission; the separate authority gate
permits one queued or active Main execution per delegation.

| Authority transition | Admission and resulting state |
| --- | --- |
| Creation | Revision zero, Main-controlled, attributed to the fixed delegator |
| Main task update | Fixed delegator and exact current revision; replace task and advance revision |
| Handoff | Fixed delegator, exact current revision and no retained execution reservation/permit; advance revision and become User-controlled |
| Main mutation after Handoff | Refuse; exact item retries still return their original receipt |

Authority applies to the whole Conversation. Partial-field merging, implicit transfer and return to
Main are not exposed. A ticket prepared before Handoff remains inspectable but cannot acquire a
permit afterward. Any unknown write freezes the entire writer authority until canonical reopen.

| Final file state | Reopen disposition |
| --- | --- |
| Complete, newline-terminated records | Validate and replay unchanged |
| Complete valid final JSON without newline | Retain its item and add newline |
| Incomplete final JSON prefix | Preserve the fragment in an owner-only sibling, then truncate |
| Truncated final UTF-8 character | Isolate only if its valid UTF-8 prefix is incomplete JSON |
| Impossible JSON prefix, invalid UTF-8, semantic error or earlier corruption | Fail closed without rewriting evidence |

COL-4 uses JRN-4's process-death boundary, with no fsync/power-loss guarantee. Replay performs no
provider, tool or UI effects.

## Evidence

[Named proofs](../evidence/collaboration-ledger.md), one row an invariant.

## Integration boundary

Runtime child binding checks the exact worker Conversation and physical execution authority.
[SCH-1–SCH-5](./owned-scheduling.md) put the file and bounded child runners behind one composition
owner, reserve runner capacity before turn admission, retain permits through owned work, and join
Stop/Handoff. [CMP-1](./collaboration-mail-projection.md) derives attributed Incoming/Sent snapshots
through that live owner. [CTL-1](./collaboration-tools.md) seals Main authorship to the exact
user-owned root runtime tool ingress before constructing events; a serialized author is not a
credential. Production routes delegation, mail and Handoff into both conversations; authenticated
controller snapshots gate the focused child input route. Accepted admission means
present in this log, not included in a model request or completed. [CIN-1–CIN-4](./collaboration-inclusion.md)
own frozen turn admission, session inclusion references and the dispatch barrier. Provider
projection must preserve a distinct semantic mail atom, with an explicit encoding or typed refusal
per dialect; this component proves no endpoint support.

[ATT-1–ATT-3](./attention.md) join producer-journal request content to reference-only
`AttentionRequested`/`AttentionResolved` records. Those records authenticate root projection and
remain outside every model-source prefix. A newly admitted live request issues an exact
runner-generation decision route; passive replay reconstructs presentation without runtime or
decision authority.
