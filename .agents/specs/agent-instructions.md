# Spec — Agent instructions

| Field | Value |
| --- | --- |
| Status | Implemented; live model adherence unverified |
| Owns | AGENTS.md discovery, scope, bounded request snapshot and conversation-open lifecycle |
| Depends on | SKL-1, WFS-1, PRV-4/PRV-6, BUD-1–BUD-4, CPL-2/CPL-3 |
| Proven by | CLI discovery/lifecycle tests and four-dialect provider tests below |

## Invariants

**AGI-1 — Discovery follows one physical project scope.** Load optional `PLEXMATON_HOME/AGENTS.md`
first, then `AGENTS.md` in each directory from SKL-1's project root through physical cwd, in that
order and once per path. Do not traverse siblings, descendants, a linked worktree's common Git
directory or ancestors above its checkout.
Rejected: filesystem-root ancestry, because nested task worktrees would inherit the primary
checkout's duplicate instructions and project configuration already has a physical root owner.

**AGI-2 — Loaded instructions are complete, bounded and attributed.** Files retain exact UTF-8
content and source/scope paths; missing files are optional, while non-files, symlinked instruction
files, invalid UTF-8/NUL, changed reads, cancellation and limit violations refuse the load. Acquisition uses
WFS-1's pinned reader, and publication never truncates a rule into apparent completeness.

**AGI-3 — Repository prose is user-level context, not execution authority.** Every codec emits the
same snapshot once before journal history in its user role, preserving configured system
instructions. The scope preamble gives explicit user requests precedence, deeper applicable files
precedence over ancestors, and directs the model to read nested files before working there;
instruction loading does not widen tools or grant permissions.

**AGI-4 — Instructions participate in the existing environment.** BUD-3 budgets their actual
encoded bytes once; BUD-2 invalidates usage anchors when the snapshot changes. CPL-2 keeps the
snapshot in the summarizer's unchanged prefix, and CPL-3 retains it outside the compacted history.
Debug and ledger diagnostics contain no loaded document text.

**AGI-5 — A conversation uses one snapshot until reopened.** Startup loads before terminal
acquisition; conversation creation/resume through the picker loads in its owned cancellable
worker before replacing the runtime. File edits cannot change a running request or compaction;
reopening reads current files, while journal replay performs no instruction read or mutation.
Rejected: storing another prompt history in JSONL, because PRV-4 already defines deterministic
replay from journal plus current resolved environment; and watchers without a user-facing reload
contract.

## Grammar and bounds

Only the exact filename `AGENTS.md` is recognized. There is no frontmatter parser, compatibility
fallback name, Git-ignore filter or interpretation of Markdown links as automatic file reads.
User-home content applies to the conversation; project content applies beneath its containing
directory. Configured authority roots and cwd are canonicalized; descendant instruction paths use
no-follow reads. Global and project files at the same physical path are loaded once.

The project root is the existing SKL-1 result: nearest valid Git checkout/worktree above cwd, or
cwd without Git. At most 128 project directories are visited. The complete rendered snapshot,
including its scope preamble and JSON source envelopes, is at most 64 KiB. Oversized source files
are refused after a bounded read; JSON escaping and source paths count toward publication too.
Empty/whitespace-only files add no content; retained nonempty bodies are not trimmed.

Nested files below cwd are not eagerly scanned or automatically injected on tool access. The
preamble instructs the model to discover and read them using ordinary tools; those reads retain
JRN-5 tool outcomes. Model adherence to prose is not a permission guarantee or an offline-test
claim. Restarting, `/new`, or switching to a saved Conversation refreshes the initial snapshot;
selecting the already-open Conversation is a no-op. Changing reasoning effort retains the snapshot.
It is request environment, not a visible user turn or journal entry.

## Evidence

[Named proofs](../evidence/agent-instructions.md), one row an invariant.
