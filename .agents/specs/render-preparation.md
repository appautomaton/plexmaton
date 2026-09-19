# Spec — Owned render preparation

| Field | Value |
| --- | --- |
| Status | Text and native math preparation connected to the live TUI |
| Owns | Shared preparation payload, process lifetime and completion admission |
| Depends on | FR-2/FR-3, MD-4/MD-5, MTH-4 |
| Proven by | TUI admission/copy tests, codec and real-process tests, the production-loop blocking witness and release measurements |

## Invariants

**PRE-1 — Preparation carries source identity, not terminal authority.** A bounded request and
reply preserve agent, entry, source revision, width, disclosure and math capability along with palette-neutral
rows, atomic formula rectangles and copy ranges. The private child dispatch occurs before configuration and terminal setup;
it reads only piped requests and returns framed data, never terminal commands (MD-4/MD-5). A streaming
Markdown request may carry a bounded, parser-signature-checked prefix layout; the complete source
crosses the boundary, where the worker validates its full-parser event pass and renders only suffix
events. Invalid hints take canonical preparation; optional hint bytes are discarded before refusing
a source that fits the request budget alone. Text fragments validate their checked column plus exact
grapheme width against the requested width; atomic formula geometry remains separately validated.

**PRE-2 — Cancellation ends computation before replacement.** One owner retains at most one
active operation and one latest pending batch, keeps partial I/O across select interruptions,
and kills and reaps its child on replacement, timeout or shutdown. Cleanup failure quarantines
replacement; idle owns no periodic wake (MTH-4/FR-1). Rejected: aborting a blocking future, which
discards a result without terminating the synchronous parser.

**PRE-3 — Completion cannot move unseen text beneath input.** Adoption matches an owned request
in the current workspace generation and its exact source/geometry identity; a successful frame
alone replaces the immutable pinned text hit map (FR-3). Missing or failed preparation is local
presentation, never permission for draw, input or selection validation to invoke a parser.
MD-4's retained presentation carries its own source identity through height measurement, paint,
native reservations and pointer mapping, even when a completion is overtaken by another delta.
MTH-5 requires both cell and native output success before that publication.

**PRE-4 — Selected-text copy waits for data, not on the input loop.** Release captures painted
fragments and retains a bounded assembly with every member's observed revision/disclosure for
cancellation and missing-entry preparation. The same owner fills unpainted gaps and emits one
complete copy (SEL-1/SEL-2/SEL-4). Captures iterate only the bounded painted set and account for
container capacity plus optional fragment allocation; an empty member is a captured absence.
A new explicit Copy captures the current painted representation. Changed members, a new
selection or another copy cancel obsolete delivery; failure and capacity limits are explicit.

## Evidence

[Named proofs](../evidence/render-preparation.md), one row an invariant.

## Model

The TUI owns pure preparation and retained presentation data. The CLI library's concrete owner is
shared by the executable and real-process measurement harness. It owns framed pipes and a
single persistent child of the current executable, with an empty environment, no terminal
handles and discarded stderr. The child performs no configuration, provider or storage startup.
Its synchronous engine is isolated so cancellation can actually end CPU work. The main process
keeps terminal output ownership. This is a private same-build protocol, not a compatibility API.
The live adapter polls that owner beside input, clipboard, runtime and frame deadlines; all exits
join it. Workspace generations are retained identity tokens, without a global counter. Frames
declare reached keys, not queued source clones. Capacity-refused batches halve to one entry before
an individual refusal becomes visible; a successful batch restores the full ceiling.

| Boundary | Policy |
| --- | --- |
| Active / pending | One retained operation and one latest encoded batch; no per-request detached task |
| Request | A hard entry count shared with the wire protocol; snapshots and prefix hints are admitted before cloning, not after; a Markdown prefix hint is capped separately and its length prefix checked before allocation |
| Reply | One bound over both the encoded bytes and the aggregate prepared allocation they carry, so a batch cannot pass the wire and fail on arrival; oversized batches return a typed refusal. The worker's own batch bound is a separate, larger figure derived from the entry budget |
| Prepared entry | The rows a message may occupy, times what a finished row costs; identity admitted separately; validated UTF-8 copy ranges, checked grapheme-width text fragments, atomic rectangle/run consistency and selection-padding bounds |
| Frame pins | The drawing and last-painted maps have their own slot and allocation bounds, deliberately separate from MD-4's LRU: a failed frame must not evict the maps input is resolving against. The allocation bound is a multiple of the entry budget |
| Selected-text assembly | One selection, bounded across member identities and text together; no truncation or delivery acknowledgement |
| Process | Absolute executable, empty environment, piped stdin/stdout, discarded stderr |
| Lifetime | One deadline covering request/reply I/O and computation together, and a second for kill/reap; uncertain cleanup retains the child in quarantine |

Values: `plexmaton-tui/src/{preparation,transcript/preparation}.rs`, the layout cache,
`plexmaton-cli/src/preparation/wire.rs`. The entry row budget is `markdown.rs`'s and the
selected-text ceiling is `workspace/copy.rs`'s, named by the invariants that own them.

Pending revisions preserve compatible prepared content and its height; cold or evicted entries
use a placeholder with a known or estimated height. Unavailable entries show a compact refusal
without guessed text ranges. Source
copy remains available, including when an individual entry exceeds preparation limits. Plain
hidden text counts borrowed row breaks; unknown rich heights remain explicit estimates until
reached. Ordinary CLI gates use the real process. CPU-only fixtures and reference measurements
explicitly pump the shared pure batch operation outside `Workspace::draw`.

FR-4 owns the measured CPU, process and live-adoption costs, including the paired batch experiment.
Terminal transport and outer input wait are excluded; those figures are not end-to-end latency
guarantees. Byte limits do not establish an OS memory guarantee.
