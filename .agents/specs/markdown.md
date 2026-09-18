# Spec — Markdown transcript presentation

| Field | Value |
| --- | --- |
| Status | Implemented; three-width frames inspected, manual terminal use remains a user check |
| Owns | Assistant CommonMark projection, bounded table layout and the shared prepared-text cache |
| Depends on | TR-1–TR-4, FR-2/FR-3, SEL-2/SEL-7 |
| Proven by | TUI `markdown` and `workspace::markdown_tests` |

## Invariants

**MD-1 — Formatting does not mutate source.** Only assistant message presentation interprets
Markdown. Original-source Copy, journal and provider requests retain exact source; pointer ranges
use SEL-2's visible-text projection. User input, reasoning, system,
warning, error and tool text remain literal. No link/image/HTML execution, file access or fetching.
Assistant math recognizes paired dollar spans through CommonMark and source-mapped `\(…\)` /
`\[…\]` outside code, HTML and link/image literals. MTH-1 owns their atomic original source.

**MD-2 — Every styled row fits the measured width.** Headings, emphasis, lists, quotes, code and
tables share a grapheme-aware wrapper. Code keeps indentation and literal syntax; tabs display as
four spaces. A grapheme wider than the entire viewport displays a replacement marker. Tables wrap cells
or use labelled values when columns cannot fit; no value disappears to make a table fit. Retained
text fragments validate their checked start-plus-grapheme width; native atoms retain MTH-1's
independent rectangle validation.

**MD-3 — Incomplete streams stay readable and bounded.** CommonMark parses the current source
prefix, including unclosed fences. Recognized unfinished math in native mode occupies one `Math…`
row until its closing delimiter arrives; finalization reveals incomplete source. MTH-1 keeps the
pending atom's exact source, including trailing newlines. Formatting is bounded on source bytes,
parse events, nesting depth, rendered rows, rendered bytes and formatting width, and a width past
that last bound uses literal source; `markdown.rs` holds every value. The row budget is the one
bound that decides how long an entry may be: MD-4's byte budget and the preparation batch bound
both derive from it in code, so an entry that fits it cannot fail either. Rejected: stating each
value here beside the code that holds it, which is a second copy with nothing forcing agreement —
this spec twice named a row bound the unstated byte budget made unreachable, refusing entries an
order of magnitude before the number the reader was given.
Tables are bounded on columns and rows. Exceeding one of those shows a named literal-source fallback
for the entry; nothing is discarded from copy or context. A math budget is different in kind: it
bounds one entry's prepared geometry, and spending it costs geometry — the formula that crosses the
budget and every formula after it keep their exact source under a named reason, while the entry is
still drawn as a document. Rejected: a separate cap on how many formulas an entry may contain,
checked in four places and enforced by abandoning Markdown for the whole entry, which sent a
645-line letter carrying four formulas more than it allowed to the terminal as raw TeX, headings
and all.

**MD-4 — Retained preparation is reused for interaction and paint.** One prepared entry may cost
MD-3's row budget times the accounted cost of a finished row; a palette-independent LRU admits a
bounded number of layout-version slots and four full entries of accounted allocation capacity, including text maps and composed style layers,
keyed by agent, entry, revision, width, disclosure and math capability. Native runs and atom maps
share this accounting; its widths follow TR-1's two-width height cache.
All transcript entry kinds share asynchronously prepared rows between painting and pointer mapping;
rich messages and disclosed tools also reuse them for measurement. Hover, selection and palette
changes do not invalidate preparation. Each geometry retains at most its two newest revisions
under the same LRU bound; finalization can leave both the last streaming and final versions retained.
While append-only text is pending, measurement and paint reuse the newest successful compatible
revision no newer than the source, pinning its own key and rows; the current revision is still
requested. Tools, artifacts and mail require an exact revision because their fields are current
facts, not text prefixes (ENT-2/ENT-3). Cached outcomes retain their key on both success and refusal;
oversized lookup identities refuse before key allocation. An exact refusal remains visible. An evicted layout is
requested when reached again or needed for selected-text copy (PRE-3/PRE-4), never rebuilt by an
input handler. PRE-1's allocation limit gives oversized entries a named refusal with source copy
intact; a finished layout releases its growth headroom first, so an entry is charged for what it
holds rather than for the room it grew through, and the refusal means the entry is genuinely too
long rather than that the allocator rounded the wrong way. Height metadata outlives layout eviction and palette replacement. `text_layouts()` counts
admitted preparation results delivered to the cache, including typed refusals and results dropped
by retention; it does not count occupied slots or parser invocations.
Streaming Markdown also retains a bounded checkpoint only after a complete top-level paragraph, heading or physically closed fenced code block. The checkpoint carries source bytes, visible-text/row coordinates and a full-parser event
signature; late reference resolution, source replacement, malformed copy maps and open structural
state invalidate it.
The owned worker receives the complete source and full-parser suffix events, while layout, native
math and syntax work before the checkpoint is reused. A completed entry takes the canonical full path.
Checkpoint source/layout hints are capped at 64 KiB and are omitted when that bound cannot be met;
the existing request/reply bounds remain authoritative.
Hidden plain prose without any supported syntax trigger keeps count-only measurement; admission never parses
Markdown or infers formatting from regular expressions.

**MD-5 — Color resolves from semantic style intent without reflow.** Prepared text retains ordered
workspace/Markdown/code role and modifier patches, never resolved terminal colors; painting uses the
current palette without parsing, wrapping or rebuilding copy fragments. The explicit Markdown
theme belongs to palette identity, independently of workspace chrome; its inherited choice follows
the workspace palette, and its designed choice names the designed slots rather than the palette's
own, so a replacement palette reaches the inherited one and not the designed one. The designed palette's Markdown names the same slots: blue, green and cyan
headings, blue links and yellow inline code. A 24-bit terminal is assumed; there is no reduced-colour
resolution and no slot fallback.

Rejected: regular-expression Markdown parsing; storing decorated text in JSONL; executing HTML or
fetching image/link targets; hiding table cells on narrow terminals; styling user instructions as
Markdown without an explicit product decision.

**MD-6 — Syntax is bounded presentation of literal code.** Recognized fenced languages prepare
semantic code roles inside PRE-1/PRE-2's owned worker, never in draw or input; unknown or unlabelled
code remains literal. A 32 KiB per-block code budget and bounded highlight events degrade a whole
block to visibly labelled plain code, preserving all text; incomplete syntax remains readable and
selection retains token distinctions. The code frame encloses the current presentation; it does
not claim a physical closing fence has arrived. Only a physically closed top-level fence can be
frozen under MD-4.

## Code theme

The fence's first info word selects Rust (`rs`), Python (`py`, `python3`), JSON (`jsonc`),
JavaScript (`js`, `jsx`), TypeScript (`ts`, `tsx`) or Bash (`sh`, `shell`), case-insensitively.
Empty, `text`, `txt`, `plaintext` and unknown info words keep plain code. This is grammar-based
syntax classification, not language-server semantic analysis. No automatic language guessing.
A whole code block above 32 KiB, more than 32,768 highlight events or more than 128 nested
captures loses only highlighting, with an explicit plain-text label. MD-3 bounds the complete
entry; PRE-2 bounds computation, replacement and shutdown. The byte and event budgets bound the
work, not its wall-clock time, which is superlinear in block size: adversarial punctuation at the
32 KiB cap costs seconds, while real code at that cap costs milliseconds. Open blocks reparse only
when their revision reaches the coalescing worker; unchanged paint never parses. This is not an
incremental syntax-tree cache. Each bundled grammar's query is compiled at most once per process
and then only read, because compiling one costs two orders of magnitude more than highlighting an
ordinary block with it.

| Code role | Pastel token |
| --- | --- |
| Text, variables, operators and punctuation | Body |
| Keywords | Blue |
| Types and properties, including JSON keys | Cyan |
| Functions and macros | Yellow |
| Strings | Green |
| Numbers and constants | Orange |
| Comments | Muted, italic |

These are content roles, not workspace attention states. Inherited palettes use Body with bold
keywords and muted italic comments, retaining a color-free monochrome path. Headings, bold,
italic, quotes and links keep MD-5's styles; inline code adds the existing Bar background.
Pointer and entry selection use the existing Bar background for pastel Markdown, preserving
foreground colors and emphasis; inherited and monochrome themes retain the workspace Selection
style. Selection padding retains its measured width. Source Copy and pointer
Copy continue to use MD-1/SEL-2. Rejected: terminal-colored spans in preparation, which would
require parsing again when the palette changes.

## Evidence

[Named proofs](../evidence/markdown.md), one row an invariant.
