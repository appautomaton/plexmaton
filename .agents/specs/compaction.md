# Spec — Compaction

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Compaction planning, immutable checkpoint provenance, collected summarizer outcomes, bounded orchestration and the user's request for one |
| Depends on | BUD-1–BUD-4, JRN-1/JRN-3/JRN-5/JRN-7, PRV-1/PRV-3/PRV-4, TIM-2–TIM-5; [context epochs](../ui-ux.md#context-epochs-and-branch-selection) |
| Proven by | Agent, provider, JSONL and runtime tests below; the [spike](../spikes/compaction/README.md) retains source comparison |

## Invariants

**CPL-1 — Planning freezes one authoritative source.** A pure plan binds one head, revision,
semantic boundary, prior checkpoint epoch and request environment; its cut names stable atom
identities on that projection. No plan writes history, starts an effect, splits a JRN-5 tool batch,
or mutates opaque replay.

**CPL-2 — Summarization preserves the complete request prefix.** One stable instruction is appended
after the exact old history under the same model, instructions, tools and output configuration.
Each operation has one input and one attempt; overflow refuses compaction instead of deleting,
flattening, truncating or rewriting history, including opaque replay.
Rejected: fitted/lossy inputs, because even temporary history changes break the required cache prefix.

**CPL-3 — The replacement preserves the current request and exact retained context.** A checkpoint
projects its persisted summary, the latest user and its attached explicit skill when covered, and the exact retained
suffix, followed by later entries. Summary size, retained context and output reserve are budgeted
explicitly; a structurally empty cut, an oversized required user input or an unfittable
environment is a typed planning refusal, never an unbounded compaction loop.

A conversation no larger than the tail a checkpoint would keep has no history in front of that
tail, and planning declines it before any summarizer call. Both quantities are this runtime's own
— a configured retention count against `utf8_heuristic_v1` estimates (BUD-3) — so the decline is a
default about numbers it chose, not a measurement of the provider's tokenizer, and the user lifts
it for one request by naming the override. Nothing past the gate changes: the same cut, the same
appended instruction, the same publication follow an overridden request.
Rejected: comparing the covered prefix against the largest summary the model is permitted to
write, which on a 272k-window model refused every prefix under 16k and so refused nearly every
real compaction.

Whether the replacement is smaller than what it replaces is not checked, and a larger replacement
publishes like any other; only fitting the input window is a condition. The runtime cannot read a
summary and cannot measure it, so a rule made of either quantity spends a finished model call to
decide a question it has no evidence for.
Rejected: refusing a replacement that does not reduce the frozen request, which paid for the
call, wrote the summary, discarded it, and reported an estimator's verdict as a failure.

**CPL-4 — A checkpoint is an acknowledged journal fact.** One additive checkpoint entry records its
versioned plan and successful summarizer-attempt identity; that attempt's full output is already
durable in the same journal. Source revision, ancestry, cut, output, owner and environment are
validated before commit; JRN-7 acknowledgement precedes replacement publication or continuation.
Original entries remain intact; JRN-3 owns the current `plexmaton.session` schema epoch and foreign-epoch refusal.
The conversation shows each checkpoint as one row where it landed (ui-ux §transcript grammar),
emitted when it commits and projected from its entry on reopen, under one identity derived from its
record, so an automatic checkpoint sits mid-turn and a requested one after the last entry.

**CPL-5 — Epochs belong to the selected ancestry.** Projection selects checkpoints only on the
target path, preserves the full visible history, and reconstructs the same context after reopen or
repeated compaction. Usage anchors require that path's checkpoint epoch and ordered projected
prefix, while historical forks preserve the original head and use their own nearest checkpoint.
Retention configuration selects future cuts; replay uses a checkpoint's persisted cut even after
that configuration changes. Rejected: reselecting an existing tail from current configuration,
original journal-position order for checkpoint context, and a source checkpoint as a historical-fork ban.

**CPL-6 — Summarization is its own model operation.** Every attempt has a real `CompactionId` owner
and distinct request-attempt identity, without a fabricated model step or durable user turn.
Bounded text, reasoning and replay output are retained with the terminal audit, including valid
partial output on failure; only complete, nonempty, bounded text without tool calls can publish a
checkpoint. Summary tool calls never reach admission or execution; TIM-3 accounts each attempt once.
Authorizing an attempt tells the conversation a summarizer started, and every terminal, published,
failed, cancelled or timed out, tells it the summarizer ended, whoever asked for it; both are live
only and never projected from the journal, so a reopened conversation is never left compacting. They
take numbers in the live stream alone, which a reopened or rebuilt projection restarts from its own
last event, so the stream the workspace reads never gaps.

**CPL-7 — Automatic work has one bounded owner.** Pre-turn soft pressure, complete post-tool hard
pressure and one empty-output typed context-error recovery are routed through the owned runtime.
At most three separate compaction operations and one agent context-error recovery occur per turn;
there is no changed-input summary retry. Cancellation, timeout, accepted writes and shutdown retain
observable completion and no detached work. If the complete history plus instruction cannot fit,
hard pressure stops the step before any summary request.

**CPL-8 — Failure preserves a usable authoritative state.** Definite planning or summary failure
does not publish a checkpoint or alter the source head; a soft failure may continue the old request
only while it remains within the hard budget. Hard failure is visible, and uncertain persistence
keeps JRN-7's freeze/reopen rule; neither retry nor model/tool dispatch crosses an unacknowledged fact.

**CPL-9 — A request is one idle attempt with nothing to continue.** `/compact` asks the runtime
for one compaction of the selected head. It is admitted only while idle: no turn, no pending
approval, no owned compaction, shutdown not begun. Every other state, a missing budget, and a plan
that finds nothing to replace or nothing that fits, is a typed refusal that writes no record.
One of those refusals is a default rather than an inability: a conversation inside its retention
window (CPL-3) is declined, and `/compact --force` asks for the same request again with the gate
removed. The decline names that override and carries the emphasis of a waiting action, because
something is waiting on the user; the refusals the user cannot lift stay out of the way. An
admitted request follows CPL-1–CPL-6 and CPL-8 unchanged: one authorization, one attempt, one
checkpoint, no fabricated step and no model call afterwards. Text submitted while it runs waits
in the runtime's bounded input queue and opens its turn once the request ends, so the first
request after `/compact` already starts from the summary; interrupt and shutdown cancel and join
the request and return the waiting text as they do queued input. The outcome, published or failed
with its kind, reaches the composition root as a report beside the attempt's visible failure.
Rejected: returning typed text to the composer while the summarizer runs, which made the user
send again what they had already said; and retrying a failed request, because the user can ask
again.

## Model

The agent crate owns semantic types, checkpoint validation/projection and the staging entrypoints.
The provider crate owns codec-based estimates and prepares one append-only request.
The runtime owns scheduling, transport, collection and commit ordering. The ordinary
agent loop still receives only its own steps; it resumes the same pending step with freshly
projected context after a checkpoint. A requested compaction takes the same path from a pure plan
to an acknowledged checkpoint and then reports instead of refreshing a step.

```text
acknowledged context -> pure plan -> Compaction attempt authorization -> ack
    -> collected model operation -> Compaction attempt finished (audit + full output) -> ack
    -> checkpoint entry referencing that output -> ack -> refreshed agent request
```

The intended narrow data surfaces are:

- `ContextEpoch`: original context or a checkpoint `ConversationEntryId`; derived from ancestry.
- `CompactionSource`: selected `HeadName`, `HeadRevision`, semantic boundary and `ContextEpoch`.
- `CompactionCut`: first/last covered atom identity, optional first retained atom identity, and an
  optional covered latest-user identity to preserve. An atom's first source entry identifies it;
  validation resolves the complete atom rather than cutting among its source entries.
- `CompactionPlan`: operation ID, source, cut, `RequestEnvironment` and maximum summary text bytes.
  It stores only bounded descriptors; the summarizer request remains a transient projection.
- `CompactionInputMode`: the durable `verbatim` audit marker; there are no rewriting modes.
- `CompactionAttemptFinished`: ordinary terminal audit, input mode, and either complete
  `AssistantOutput` or a typed failure with optional valid partial output. This is one new
  non-advancing `JournalRecord`; existing record variants gain no required fields.
- `CompactionCheckpoint`: versioned plan plus successful `RequestAttemptId`. Its context summary
  is a distinct semantic atom, encoded as harness-supplied user context by each existing codec;
  summarizer reasoning/replay stays in the attempt record and outside the visible transcript.

Checkpoint coverage uses stable range endpoints against its frozen source projection. It does not
duplicate all covered entries, original text or tool outcomes into a second history. Source
selection and complete-atom validation make missing, reordered or foreign provenance a typed error.

The default planning policy reserves up to one quarter of available input for summary text,
subject to the model's existing reserve and a 64 KiB summary-text cap. Each model's
`compaction_keep_recent_tokens` defaults to `20000`; its effective suffix target is the smaller
of that value and one quarter of input capacity after environment occupancy. Selection retains
whole newest atoms within that target and preserves the latest user exactly. A covered latest user
is pinned after the digest. The target is approximate: atoms are never split and required user
content is not truncated. Its immediately attached SKL-5 activation is part of that required context;
both preview and journal projection retain and budget the same pair. Older activations may be summarized.
This policy controls checkpoint projection, never the summarizer input.
Zero is rejected. The setting does not enter wire encoding or the environment fingerprint;
changing it leaves existing checkpoint context unchanged, including after reopen and subsequent
user turns. Only a new compaction selects a new cut under the new target and starts a new epoch.
Impossible required context fails explicitly. Actual request encoding is estimated by
the same BUD-3 implementation as ordinary context. A heuristic fit is not a provider guarantee;
publication also checks the actual resulting context and useful reduction. Reduction is measured
with codec token estimates, not atom count: replacing a large tool batch with a summary can keep
the same number of atoms. The journal validates structural coverage independently of that estimate.

Failed summarization can retain reasoning/replay for audit without making it a continuation token.
Only the accepted summary text and original retained context enter the replacement; the provider's
session cache-affinity hint is independent of the semantic checkpoint identity.

The appended [prompt](../../crates/plexmaton-provider/src/compaction/prompt.md) requests a concise
state handoff: objective/constraints, current state, open work and essential references. It carries
forward applicable earlier summaries, distinguishes observed results from assumptions, and keeps
exact identifiers needed for continuation. Essential facts stay in the digest even when recent:
the prompt cannot assume which facts the bounded tail retains. It records the exact continuation
point, applies the latest user corrections and keeps next steps within unfinished requested work,
without reviving completed or abandoned tasks. Tools stay advertised,
while the prompt requests no tool use and CPL-6 rejects any returned calls. Prompt wording is reviewed
offline; cache hits and model-generated summary quality are not live-test requirements.

## Evidence

[Named proofs](../evidence/compaction.md), one row an invariant.
