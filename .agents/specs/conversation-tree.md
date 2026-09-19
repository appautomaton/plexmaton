# Spec — Conversation tree

| Field | Value |
| --- | --- |
| Status | Implemented; connected message graph and fold summaries verified locally and user-reviewed |
| Owns | Semantic tree projection, durable navigation, tree modal and draft return |
| Depends on | JRN-1–JRN-8, TIM-1, CPL-5, COM-3/COM-6, CMC-2, INV-1/INV-3/INV-6; [context epochs](../ui-ux.md#context-epochs-and-branch-selection) |
| Proven by | Agent, runtime, store, TUI and CLI tests below; real PTY smoke |

## Invariants

**TRE-1 — One action opens one modal.** `/tree` and `/rewind` resolve to the same typed action.
The `Conversation tree` modal owns its cursor and any enabled search input, blocking input below it while
producer events continue. Native frames are locally inspected; the user accepted the interaction on 2026-09-13.

**TRE-2 — Tree rows are semantic projections.** Stable entry identities and parent relationships
define the tree; record order supplies chronology, not lexical IDs. Audit records are not messages;
previews and any enabled query results are bounded and truncation/partial availability is explicit.
The snapshot keeps append chronology; display traverses contiguous subtrees in chronological sibling
order and adds a lane only at a split; malformed ancestry refuses partial display. Ineligible tool
batches are omitted from the message list; retained rows connect through their nearest retained
ancestor. Eligible completed tool batches and read-only context rows remain available. Answer text
precedes reasoning in preview selection; exact source and canonical snapshot metadata stay intact.
Rejected: directory-depth indentation for every sequential step, and chronological flattened rows
with depth-first connectors, which misrepresent linear work and interleaved branches; intermediate
tool fold controls, which hid final answers behind non-rewindable implementation steps.

**TRE-3 — Rewind preserves the original continuation.** Rewind creates and selects a fresh named
head in one journal mutation at a validated stable boundary. Selecting an existing head moves only
the durable selection; rename preserves selection identity and abandoning the selected head refuses.

**TRE-4 — Navigation commits before becoming visible.** Runtime admission revalidates origin,
revision, target and idle state; JRN-7 acknowledgement precedes projection replacement and draft
return. Busy/stale/refused navigation starts no work, and an uncertain write requires reopen.

**TRE-5 — Draft restoration preserves user input.** User targets return exact text and historical
explicit skill binding; resubmission follows normal current skill preparation. Like Pi, an existing
nonempty composer draft takes precedence; otherwise install the returned pair without concatenating.
Browsing, cancel and failed navigation preserve the original draft, binding and focus.

**TRE-6 — Selection follows identity and focus.** Hover and keyboard share a tree cursor separate
from transcript text selection. Filtering, folding, child dialogs and resize retain a valid selected
ID or a defined visible ancestor; empty/root results are explicit states, never negative indices.
Initial selection resolves the active head through the same semantic marker as its badge, including
hidden journal tips and folded ancestors. Keyboard, pointer and folding share display traversal and
retained parents. Only retained descendants make a row collapsible; `[+]` expands and `[−]` collapses.
The complete three-cell control is clickable. Each message occupies a node row and a non-interactive
connector row; linear continuations connect vertically in one lane and forks add a lane. A leaf is
`•`, not a fold control. A collapsed node replaces its connector with the complete retained-descendant
count and hidden head names, prioritizing the current branch. Nested folds do not reduce that count.
Folding changes neither the canonical entries nor any head. Ineligible selection advertises Read-only, emits no
navigation on Enter and does not enter a refusal/retry loop.

**TRE-7 — Context follows destination ancestry.** A user target is before its turn; a completed
assistant target includes that output. A complete tool batch is indivisible. Rewind performs no
provider/tool replay, starts no turn, and applies CPL-5 at the destination rather than the source.

**TRE-8 — Labels and copy use semantic source.** Labels attach to stable node IDs without becoming
model messages or navigation endpoints. Copy obtains bounded exact source text, never the decorated,
shortened tree row. Label mutations must invalidate tree snapshots without breaking context accounting.

## Implementation map

Paths below are relative to the checkout. Backend and native boundaries are implemented. Core vocabulary never imports the journal, runtime or terminal.

| Owner | Files | Responsibility |
| --- | --- | --- |
| Shared vocabulary | Core `conversation_tree.rs`, `tree_snapshot.rs`, `tree_edit.rs`, `tree_source.rs` | Immutable origins, rows and addressed navigation/edit/copy requests |
| Journal | Agent `journal.rs`, `journal/heads.rs`, `journal/tree.rs`, `journal/tree_edit.rs`, `journal/tree_source.rs` | One durable selection; bounded semantic projection; annotations and exact source |
| Navigation planning | Agent `journal/navigation.rs`, `record/navigation.rs`, `turn/navigation.rs` | Canonical stable-boundary validation, destination projection and returned draft; no retry or turn start |
| Runtime | `runtime/tree_admission.rs`, `runtime/navigation.rs`, `runtime/tree_edit.rs`, `runtime/transition.rs` | Shared idle/origin fence; owned nonblocking commit; acknowledgement-gated receipts |
| UI state/rendering | `crates/plexmaton-tui/src/surface.rs`, `layout/registration.rs`, `render/mod.rs`, `workspace/hover.rs`, `workspace/pressed.rs` | One tree state and modal surface using native layout/hover/press machinery |
| Input/composition | `crates/plexmaton-tui/src/router.rs`, `state/composer_menu/grammar.rs`, `state/composer_menu.rs`; `crates/plexmaton-cli/src/input.rs`, `interaction.rs` | Alias-aware command parsing/completion, modal routing, runtime result conversion |

TUI takes no agent or runtime dependency, which `check-crate-graph.sh` enforces. `ConversationId`,
`AgentId`, `ConversationEntryId` and `HeadName` own identity; a tree revision token crosses the
boundary rather than agent's `HeadRevision` and `JournalSequence`.

## Data boundary

Use these logical fields; serialization belongs only where data is actually durable:

- `TreeOrigin`: conversation, displayed agent, selected head and a snapshot revision derived from
  the complete acknowledged journal sequence. This conservative token rejects stale trees even
  when only an audit/label changed. Do not invent a second mutable version counter in the UI.
- `TreeRow`: entry ID, nearest visible semantic parent ID, chronological ordinal, row kind,
  bounded preview, optional label, head markers, active-ancestry flag and typed navigation eligibility.
  Group assistant blocks and complete tool batches; map each displayed row back to a canonical
  boundary. Do not duplicate shared ancestors by walking every head independently.
- `TreeSnapshot`: origin, rows and heads; acquisition errors carry typed limit diagnostics. Build from acknowledged
  state on demand, not on every streaming delta. Presentation caches are disposable projections.
- `TreeNavigation`: origin plus `Rewind(entry_id)` or `SelectHead(name)`. Rewind resolves the actual
  boundary in the agent, never from caller-supplied role/parent data. Head management has explicit
  typed operations; do not parse display strings back into commands.
- Agent `TreeNavigationResult`: selected head, optional mutation sequence and returned text/skill
  pair. `Reaction`/`DispatchReport` carry a dedicated receipt beside `projection_reset`;
  `TreeEditResult` carries the new origin without a reset. Neither is an `UndeliveredInput`.

Snapshot acquisition is bounded to 64 active heads with at most 256 UTF-8 bytes per head name
(checked before cloning even for older journals), 16,384 unique ancestry entries, 65,536 scanned
journal records, 2,048 semantic rows, 512 UTF-8 bytes per preview including its truncation marker, and 256 KiB of aggregate preview
text. Crossing an acquisition limit returns a typed error and no partial snapshot; shortening an
individual preview sets its explicit truncation flag. Label and edited branch names are bounded to
256 UTF-8 bytes, reject whitespace-only text and control characters, and preserve accepted text.
`None` clears a label; equal metadata is a no-op. Query limits are needed only if search is included.
No silent partial tree or clipped rule that reads as complete. Start with bounded in-memory
projection and visible-row rendering; a database/index or async paging framework is not authorized
by hypothetical scale. Exact copy has an 8 MiB assembled-byte limit including separators and refuses
oversized or incomplete source rather than shortening it. Text/reasoning blocks remain in source
order; tool groups include each tool name, raw arguments and terminal output/failure, joined by
blank lines. Source bytes within each part remain unchanged. Opaque provider replay is excluded;
reused tool-call IDs are resolved by canonical assistant ancestry, never a global last match.

## Journal operations

`ForkAndSelectHead` is one journal mutation containing normal sequence
and record identity, expected source head/revision, fresh destination name and destination entry.
Validation checks source ownership/revision, name availability and stable target before changing
anything; reduction creates the new head and selects it together. Generate names from journal
sequence and check availability, never from a UI row number or wall-clock-only guess.

`SelectHead` names an existing destination, with expected selected origin and destination
revision. Runtime first compares TreeOrigin against current acknowledged state. Rename updates a
selected head in the same mutation; refuse abandoning a selected head until another is selected.
`SetEntryLabel` changes annotation metadata only, not model context, head revision or entry payload.

The selected head belongs to ConversationJournal and is reconstructed from records. `Record`
derives its head from that owner; `Record::from_journal` projects the durable selected head.

Old journals without selection records default to their existing main selection. Preserve healthy
old bytes/header and supported epochs under JRN-3; additive records do not justify header rewrites.
Test old healthy journals and active-head rename/abandon interactions explicitly. Blank automatic
sessions still materialize only on accepted input: opening an empty tree writes nothing and offers
no rewind target. Metadata edits on an empty semantic tree refuse before staging a write; this
preserves AutomaticJournal's bootstrap exception rather than widening it.

## Navigation state machine

| State/event | Required transition |
| --- | --- |
| Open tree | Snapshot acknowledged state; retain draft/binding/return focus; no model or journal effect |
| Fold/hover; search/filter if enabled | Update only tree presentation; stable selected identity and scroll |
| Close before admission | Dismiss and restore underlay/input; no navigation mutation |
| Enter while busy | Typed Busy; tree stays usable; do not interrupt live work implicitly |
| Enter stale/invalid target | Typed refusal; retain draft and selection where valid; refresh snapshot explicitly |
| Valid idle Enter | Stage one owned mutation and pending result; show pending state, not new context |
| Journal acknowledges | Publish replacement then returned draft; close overlay; normal submission remains a separate action |
| Definite/unknown write failure | Reuse JRN-7 freeze/reopen rules; publish no successful navigation and lose no draft |
| Close/interrupt/shutdown after admission | Accepted writes stay owned and joined; do not claim rollback. dismissal preserves the owned write |

Idle excludes an active agent turn, pending approval, agent/runtime queued input, model/tool work,
compaction, skill preparation and pending commit. The runtime owns one navigation-admission
predicate shared with metadata edits over these existing owners, also refusing shutdown,
journal-frozen state and an unconsumed tree receipt or projection reset.
`has_active_work()` alone is insufficient: a tool admission worker can finish while the agent
remains in `Turn::Working` awaiting approval, so admission combines it with the agent turn and
queue state.
Selecting the already-current destination is a no-op. User rewind preserves the historical explicit
skill selection, including numeric names; it does not read current skill files until resubmission.
An unsent returned draft need not persist across process exit; the selected branch/context must.

## Native interaction

The tree fills the content rectangle above the existing Status/quit row. The Drawer can open
above it without losing the tree's cursor or draft. The single key/pointer grammar is owned by
[interaction routing](./interaction-routing.md#key-grammar): clicks select, Enter navigates,
child editors own their text, and dismissal never claims to roll back an admitted write.
The native presentation keeps linear steps aligned and reserves a right-side badge for named heads;
`●` marks the current branch even when multiple names share one display anchor. Canonical branch
selection and copy still resolve the original head tip. Controls and the active head use `Accent`,
connectors and inactive heads use `Muted`, and previews retain `Body`/`Muted`. Selected row headings use
`Chosen`; its background spans the row without overwriting the other semantic foregrounds or weights.
The footer advertises the selected row's action and names expand/collapse only when available.
Scroll offsets/capacity remain semantic in UI state; the renderer maps to two-line node blocks.
Fixed headings do not count as scrolled rows, and connectors, summaries and spare partial rows
cannot become pointer targets.

Search and branch summaries are not exposed. The user chose the full-viewport modal, aliases,
close button, shared selection and Pi-style folding/labels; native visual evidence is below.

Branch summaries are not implemented. A summary would be model-generated context from the old
branch to the common ancestor, added at the destination — neither a tree label nor a compaction
checkpoint — so no control may advertise one until that mechanism exists.

## Evidence

[Named proofs](../evidence/conversation-tree.md), one row an invariant.
