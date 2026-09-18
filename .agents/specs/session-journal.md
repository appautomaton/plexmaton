# Spec — Session journal

| Field | Value |
| --- | --- |
| Status | Implemented and accepted through real CLI process-kill recovery, including permanent cancellation of process-dead approvals |
| Owns | Typed journal, heads, projections, wire, writer, recovery and commit/effect ordering |
| Depends on | PRV-3/PRV-4 for model replay, ENT-1/ENT-3 for transcript identity and pure reduction |
| Proven by | Agent, store, runtime and composition tests |

## Invariants

**JRN-1 — One record is one complete mutation.** An append carries its parent, head and expected
revision together, so it advances that head or changes nothing. Move, rename and abandon are also
revision-checked; create requires a fresh, never-reused name. Sequence, record and entry identities
never repeat. Terminal records check their head/boundary but advance neither.

**JRN-2 — Reduction is deterministic and typed.** Equal ordered records build equal entries, heads
and paths. Gaps, unknown ancestry, stale revisions and reused identities are typed refusals that
mutate nothing; model steps are one-based and contiguous within a turn; turn terminals additionally
reject missing, duplicate or mismatched ownership.

**JRN-3 — The wire is lossless without weakening opaque replay.** Records use tagged JSON decoded
through validating constructors. One `AssistantOutput` retains ordered text, reasoning, calls and
invisible replay-only parts;
its block-anchored replay carries route owner, codec revision and model family. Serialization keeps
exact `ProviderReplay` bytes; `Debug` and decode errors reveal none. The header names
`plexmaton.session`, a supported schema epoch, session identity and externally observed
`created_at_unix_ms`; file and in-memory journal retain the same metadata. New files use
`2026-09-05`; `2026-09-04` files remain readable and appendable through the same validating
record decoder. Skills, permission evidence, CPL-4 checkpoints, CIN-2 collaboration turn starts and
reference-only collaboration placement links are additive record variants. A placement link
validates an announced agent on its selected ancestry; session composition additionally binds that
agent to the canonical endpoint before its reference resolves through the collaboration log.
Opening a healthy journal preserves its original header and bytes; unsupported epochs fail before
record decoding. Rejected: relabeling fixtures or rewriting user journals to accommodate an
additive feature, and separate decoder implementations for identical record shapes.

**JRN-4 — One record is one write, and loading keeps a valid prefix.** The adapter uses one
unbuffered `write` per record and no `fsync`. A completed file append survives process death; power loss
may discard the unflushed tail. Load repairs complete final JSON without a newline, isolates an
incomplete tail, and stops at earlier corruption. Journal, staging and isolated-tail files are
owner-only. User-selectable root session names resolve beneath an owner-only
`PLEXMATON_HOME/sessions`. Delegated child names resolve beneath the separate owner-only
`PLEXMATON_HOME/delegated-sessions`; each directory issues a distinct, non-interconvertible runtime
token, so raw journals and the other directory cannot enter its runtime constructor (CHB-3).
Writer drop explicitly unlocks before closing its descriptor, so a transient duplicate inherited
by a concurrent fork cannot prolong writer authority until that child reaches exec.
Normal startup allocates an automatic identity without creating a directory or JSONL. The writer
retains exactly one bootstrap announcement; the first `TurnStarted` or CIN-2 collaboration turn start creates the file and writes both records
before acknowledging the input. Extra pre-turn mutations are typed refusals.
Explicit `create` still reserves a file immediately; `--ephemeral` never persists.
Automatic identities are portable UUIDv7 names while `created_at_unix_ms` remains independently
injected chronology; explicit portable string identities remain valid. Rejected: timestamp-only
automatic identities, which collide when stores combine.
Rejected: per-record `fsync` or macOS `F_FULLFSYNC` for a stronger power-loss promise; and a
database or on-disk index before a measured query need.

**JRN-5 — Replay performs no effects.** One head derives ordered, indivisible `ContextAtom`s and a
numbered event stream without invoking providers, tools, policy or files. Model-facing tool results
share execution's `MAX_TOOL_OUTCOME_BYTES` ceiling (1 MiB), independent of ENT-4's transcript-preview
limit. A complete assistant output plus every result is one `ToolBatch`; results follow call order despite out-of-order terminal
transitions. An incomplete final batch stays visible, is wholly omitted from provider input and
yields typed recovery; before a later model fact it is corruption.
Resume settles orphaned calls with a stable `process_died` result and marks the turn interrupted
before new work. Explicit turn terminals determine completion: a failed or cancelled turn with no
assistant response does not require recovery. Successful resume places **✓ Conversation restored.** in green after the restored
conversation's final entry and any recovery warning; a genuinely unfinished prior turn adds, in
yellow, **The previous turn didn't finish. You can continue from here; no model requests or tools
were rerun.** Both are UI only (ui-ux §responsive interaction); neither diagnostics nor that
confirmation enter model context.
Completed messages normalize transport chunks into one replay delta.

CIN-2 keeps collaboration inclusion as a canonical reference, and CIN-3 resolves its transient
context from the collaboration log. Replaying the session alone preserves the reference; it does
not invent source text or start an execution. Selected-branch replay uses acknowledged placement
links, falls back to a resolved inclusion boundary for older recipient journals, and retains any
remaining shared rows once in a stable collaboration-order suffix. Restoration projects those rows
before its UI-only completion confirmation.

**JRN-6 — Completed live facts enter once.** Each settled fact enters as one `JournalRecord` returned
in its `Reaction`; requests read only the selected path. Streaming deltas use a delivery cursor, but
only the completed message is canonical. Rebuilding idle state rebases that cursor; active transient
work refuses rebuilding.

**JRN-7 — Acknowledged records precede dependent effects.** The runtime stages each
transition through one bounded writer, publishing events and starting effects only after appends
return. The automatic startup announcement is the sole in-memory exception defined by JRN-4;
user input and dependent effects never cross that boundary without file writes.
Cancelled waits stay owned; shutdown joins accepted appends and the writer. Failure returns
text in arrival order, distinguishes unwritten from unknown outcomes, reports cleanup and requires
reopen. A separately durable collaboration projection is refused without numbering while a commit
is pending; the composition retains one selected activity and bounded owners hold later activity.
Definite or uncertain failure keeps the owned projection out of the failed runtime. Reopen rebuilds
facts backed by the collaboration log or child journal; shutdown reports a transient runner outcome
that has no durable source. The TUI loop performs no filesystem operation. Permission
decision provenance follows [permission-policy](./permission-policy.md) PER-9 and never becomes
replayed authority.

**JRN-8 — Retry extends context; editing preserves its old branch.** Only an idle, rate-limited
tail with no retained assistant output, reasoning or tools is eligible. Its turn identity and head
revision address the action. `TurnRetried` opens a fresh execution with no user atom; ordinary
submission adds a user atom on the same head. Edit/retry archives the old head, moves before its
question, and appends edited input. Earlier facts and incurred attempts remain immutable.
JRN-7 gates both paths; a replacement UI projection precedes its dependent events. Rejected:
duplicating the question on retry, deleting failed attempts, or automatically repeating effects.
Provider failures, including HTTP 529/5xx, do not qualify for this rate-limit action.

## Evidence

[Named proofs](../evidence/session-journal.md), one row an invariant.
