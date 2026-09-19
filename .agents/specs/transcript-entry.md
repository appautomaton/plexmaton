# Spec — Transcript entry

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | Stable transcript identity, first-appearance order, entry revisions, tool lifecycle updates, typed presentation, and replay reduction |
| Depends on | [transcript-layout](./transcript-layout.md) TR-1 and [tool-admission](./tool-admission.md) APV-5/APV-6 |
| Proven by | `plexmaton-core::transcript`, `plexmaton-agent::{record,tools,turn}`, `plexmaton-file-tools`, `plexmaton-command`, `plexmaton-runtime`, and `plexmaton-tui::{content,transcript,state}` tests |

## Invariants

**ENT-1 — First appearance fixes identity and order.** The producer assigns every transcript fact a
`TranscriptItemId`; text, tools, artifacts, mail, warnings and errors share one per-agent ordered
projection. Whatever one session addresses to another — a letter, a task Main assigned — enters both
of the conversations it names, as one item each, and each side says in a word what its own row is:
sent to, received from, assigned to, assigned by, naming the other end and never the conversation
reading it. The row and copied source use that endpoint's display label; its `AgentId` remains
routing and persistence identity. Rejected: one item filed under the producer alone, which left a delegating agent
answering a question the user could see no trace of having been asked; an arrow relative to the
reader, which the person who asked for the feature read backwards on both sides; and dropping
direction for a symmetric `from -> to`, which removed the ambiguity by removing the fact. An
acknowledged collaboration reference fixes that side's first appearance without copying the shared
body from the collaboration log. A selected branch without such a link retains the row once in a
stable collaboration-order suffix, because missing evidence cannot establish an earlier position.
A tool's
`ToolCallId` and other domain IDs correlate facts but never choose their position, and a vector
index is not an identity. A user's turn and an agent's turn carry no heading word: what separates
them is the margin, a bar down the whole height of the user's turn and plain ground for the agent's.
The other kinds keep a named heading, because `reasoning`, `system`, `warning` and `error` are not
positions in a conversation but things the reader has to be told — ambient heading and muted body,
muted, action-required, and failure respectively. Each distinction is a colour, and carries a name
as well, so a reader scanning a long transcript can find the kind they want without reading it. Provider replay metadata never enters this vocabulary (PRV-3).
Terminal newline-only rows in reasoning are hidden before the standard entry separator, during
streaming and after finalization; internal line breaks and SEL-2's original source remain intact.

**ENT-2 — One tool call is one revisioned entry.** A call first appears queued at revision zero and
each accepted lifecycle transition advances exactly one revision on the original entry. Display
order is first appearance, execution completion order is event order, and model result order stays
the batch's model-call order (APV-5). Rejected: moving a completed call to the tail or using its
completion position as model order.

**ENT-3 — Replay is pure reduction.** Feeding the same ordered envelopes to a fresh projection
produces the same state and can only mutate that projection; it cannot contact a provider, consult
policy, request approval, or execute a tool. Sequence gaps and invalid identities, revisions,
correlations or transitions are retained in the bounded notice log rather than becoming transcript
facts. A restored pending approval remains subject to APV-6.

**ENT-4 — Open and copy project retained semantic detail.** Command invocations retain typed original
shell source, working directory and timeout; [APD-1](./approval-inspection.md) owns their inspection.
 Tool presentation distinguishes text
from a canonical diff, distinguishes omitted bytes from empty text, and keeps invocation separate
from outcome. Admission contributes a bounded canonical invocation; every later lifecycle update
retains it, and execution or a no-run terminal contributes a bounded outcome. Plain text retains
at most 64 KiB with explicit omitted-byte metadata. JRN-5 owns the separate model-outcome bound. A successful exact edit retains its complete
canonical patch under the bound derived by MUT-6; unchanged file bytes never enter it. Slice 2
proves production and retention bounds. Mail discloses the same way, because a summary is whatever
another session wrote and the first real one filled the conversation it arrived in. Its compact row
is a preview: what the letter *says*, taken from the same parser that draws its body so no heading
marker, bullet or fence reaches the row as characters, flowed onto one row and cut to the width
being drawn, with an ellipsis whenever that is not all of it. A letter therefore always discloses —
the body under it is the only place the letter exists as written, and whether the preview happened
to fit is a property of the frame, not of the entry. **That body is prose, so the transcript's own
grammar draws it**: Markdown (MD-1) and native math (MTH-1) reach a letter exactly as they reach an
assistant message, and copy still carries the exact source. The bound named on mail is a size
limit, not a demotion to metadata — a letter that arrives as a document has to read as one. The
body keeps its gutter on every row a line wraps onto, so continuation reads as part of the letter
rather than as the conversation around it; `Layout::append` re-bases its copy ranges and formula
geometry onto that gutter so selection and atomic formula copy survive the indent.
Rejected: reserving disclosure for tools, which silently made `Ctrl-O` inert for the one entry kind
whose payload has no other bound; a fixed heading budget, which left a preview two thirds empty on
a wide terminal; a constant floor under which a letter was deemed always to fit, which showed
an ellipsis in a panel narrower than the floor and then refused to answer it; and drawing the body
as an envelope's retained source, which was right while a simulator wrote one-sentence summaries
and wrong the moment a model wrote headings, emphasis and display formulas — those reached the
terminal as the characters the author typed, and a second parser beside the envelope would only
have drifted from the one the transcript already owns. Disclosure is view
state keyed by `TranscriptItemId`, never
another session fact: it survives lifecycle replacement, changes one cached height, and expands
inside the parent conversation rather than creating a nested viewport. `Ctrl-O` addresses the
selection's moving end; a click toggles the addressed entry without selecting or copying it,
while a drag remains source selection and hover is visual only. Copy returns the retained invocation then outcome without the disclosure's
headings, gutters, clipping, styling, or omission label. A canonical diff keeps its original
markers: added and removed lines use new-information and failure roles, hunk headers use accent,
and the patch envelope uses muted. Selection adds its common treatment without erasing those roles. The
renderer makes only bounded line-prefix decisions; unknown forms remain exact plain text.
Rejected: automatic selection on disclosure, which applies selection paint to a reading action.

## Model

```text
producer counter ─▶ TranscriptItemId ─▶ first appearance ───────────┐
                                                                  ▼
tool lifecycle ─▶ same id + next revision ─▶ pure ViewState reducer
                                                                  │
domain identity ───────────────────────────── correlation only ────┘
```

Text deltas and finalization use the same revision rule as tool transitions. Terminal entries such
as mail and artifacts stay at revision zero because they have no update vocabulary yet.

## Failure modes

| Situation | Response |
| --- | --- |
| An entry identity appears twice | Reject the later event and retain a notice |
| An entry identity is presented under another agent | Reject the event; its original owner remains authoritative |
| An update skips or repeats a revision | Reject it without mutating the entry |
| A tool entry changes call identity or label | Reject the correlation change |
| A tool skips its lifecycle or leaves a terminal state | Reject the transition |
| Sibling tools complete out of order | Update both original positions; preserve batch result order separately |
| The same envelopes are replayed into a fresh projection | Reconstruct equal state without effects |

## Evidence

[Named proofs](../evidence/transcript-entry.md), one row an invariant.
