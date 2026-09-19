# Spec — Native math layout

| Field | Value |
| --- | --- |
| Status | Live native conversation math implemented; direct-Kitty appearance approved 2026-09-06; portability and source reveal remain unproven |
| Owns | Delimited formula source, engine admission, native reservations and terminal composition |
| Depends on | MD-1/MD-3/MD-4, SEL-2, FR-2/FR-3 |
| Proven by | Rust corpus, real preparation child, atomic selection and output tests, three-width workspace frames and owned direct-Kitty CLI check |

## Invariants

**MTH-1 — Formula source is atomic.** A formula owns its complete original UTF-8 source, including
the original paired delimiters; every cell of its reserved rectangle, including blank and edge
cells, resolves to that whole source. Rejected: subexpression or bare-body copy, because an
accidental partial intersection must not produce partial TeX (SEL-2). Clicking copies and highlights
that source; either drag direction expands every intersection, including a blank edge cell, to the
whole formula. Streaming completion or Markdown reinterpretation invalidates an obsolete atom.

**MTH-2 — Native projection preserves mathematical meaning.** Positioned engine glyphs and rules
become disjoint cell reservations with admitted font mappings, parser-proven single-base combining accents, CJK text, script sizes and opaque/inherited
paint; unsupported output, collisions and indivisible width overflow are typed refusals. No
term-dropping, private-use glyph leakage, guessed negation or silent color substitution (MD-4).
Every primitive declares the extent it occupies in the coordinates it is drawn in, so the column
solver can read that extent without knowing the kind; a term placed beside an expression is never
given a cell that expression already owns. Rejected: storing a thin rule at its centre, which let a
neighbour share a box border's column and refused every framed result written with a full stop
after it — a projection defect that a collision refusal had been presenting as an unsupported
construct.

**MTH-3 — Geometry has a retained origin.** A layout owns one immutable run list and origin shared
with its exact source owner; viewport slicing cannot restart it at a different baseline (MD-3).
Only complete, visible, unoccluded native runs reach terminal output. A bisected multicell retains
its origin and atomic source range, with a visible `⋮` and `Math clipped` notice; it never restarts
at the viewport edge or overwrites another surface.

**MTH-4 — Resource ownership precedes integration.** Admission bounds source bytes and expanded
nodes/depth, and projection bounds primitives, dimensions and cell allocation before painting.
The PRE-2 child owns synchronous parsing/layout and terminates on cancellation; immutable native
runs enter PRE-1's validated reply and MD-4's accounted cache. Synchronous preparation never enters
draw, hit testing or the TUI input loop (FR-2); byte limits are not an OS memory guarantee.

**MTH-5 — Native text and cells commit one frame.** One CLI output owner serializes cell diffs,
native text and clipboard effects; native reservations and hit maps commit only after the complete
frame succeeds (PRE-3). Startup cursor measurements must prove width and scaling before native
output; unsupported, unverified and multiplexer cases visibly retain source instead of assuming
capability from a terminal name.

## Evidence

[Named proofs](../evidence/math-layout.md), one row an invariant.

## Model

Original delimited source → RaTeX parser/admitted tree → RaTeX layout/display list → owned native
scene → monotone cell projection. The dependency boundary is private; no replacement TeX parser,
first-party fraction-layout engine or source reconstruction remains. The
[dependency audit](../standards/rust.md#audited-foundation) owns package admission.

`Formula::parse` accepts one complete `$…$`, `$$…$$`, `\(…\)` or `\[…\]` span. It is not a Markdown
recognizer: MD-1 owns syntax recognition outside code, HTML and link/image literals. MD-3 keeps
unfinished recognized formulas in one pending row during native streaming, with exact source in
its atomic copy range. Finalization reveals incomplete source; completed formula refusals keep a
local label and source. Dollar spans follow CommonMark's math-extension grammar. Inline boxes share the prose axis;
display boxes occupy their own band. Table cells retain the same atomic ranges when a narrow
grid becomes labelled values. Native runs carry full, 0.7, 0.5 or two-row large-glyph sizing, font treatment and paint;
palette inheritance does not require geometry changes. Admission normalizes absent frame paint
before upstream layout, keeping explicit black distinct. Only the verified private-use negation
overlay plus equals pair maps to `≠`. A parser-labelled `\left(`/`\right)` pair is the one path
exception: the pinned generator's tall-parenthesis command signature, fill, semantic count and
bounded geometry must all agree before it becomes the existing delimiter projection. Any mismatch,
other path, font, scale, background fill or unadmitted effect refuses explicitly.

Root indices move as complete engine-owned layout subtrees into the engine's reserved index band
before flattening. Internal index spacing, scale and paint remain intact. The layout-box traversal
is bounded by `MAX_NODES`; full-size short roots may use the two-row radical glyph, while script
roots retain the piecewise projection. Rejected: associating indices with roots by horizontal
position and scriptscript size, which moved unrelated numerator scripts into denominator roots
and collapsed compound indices onto one cell.

An accent over one atomic base combines before cell reservation exactly when the decoder holds a
combining mark for the glyph the parser resolved — circumflex, macron, dot, diaeresis, tilde,
caron, breve and ring — and admission asks that one table rather than keeping a list beside it.
Literal accent-marker glyphs and wide, multi-base or structural accents refuse before layout, as
does `\vec`, whose arrow is a drawn path with no mark to combine. Rejected: a second list of
admitted labels in the engine, which let `\dot` parse, lay out over its base, and be refused three
stages later as a cell overlap — taking the whole formula it appeared in down with it. CJK
glyphs use the terminal font; the adapter asks the pinned engine for their text-glyph metrics at
the admitted style instead of guessing widths from Latin math fonts. This admits its Chinese,
kana, Hangul and fullwidth text path; explicit CJK font treatments, arbitrary Unicode font
fallback and complex shaping remain unsupported.

The public constants own limits: 8 KiB original source, 4,096 expanded nodes, 4,096 engine
primitives, 512 per native dimension and 32,768 reserved cells. Upstream logical nesting is limited
to 32; post-parse admission also checks expanded depth 64. MD-3 admits at most 256 formula boxes per
entry; PRE-1 bounds their aggregate reply and retained allocation. Native line wrapping is not implemented: the
unchanged wide structural fixture needs 91 columns and refuses at 88/60 rather than losing terms.

Each terminal frame admits at most 512 runs / 256 KiB; excess visible runs carry `⋮` and `Math limit`.
Each run is printable UTF-8, at most 4,096 bytes. OSC 66 script width is at most seven cells;
two-row runs reserve an even width of at most fourteen. Unsupported scale/width combinations are
local formula refusals before IPC, not malformed terminal commands. Palette and selection resolve
after geometry; explicit mathematical RGB and font treatment remain explicit.

The CLI probes through Crossterm's shared reader before creating its event stream, retaining
interleaved keys. Each ordinary missing cursor report has Crossterm's two-second timeout; this
does not prove a bound under persistent OS read errors. tmux/Screen deliberately use labelled
source; passthrough scaling and SSH portability are unproven. Old multicells are cleared from
their top row before row-ordered cell output; unchanged reservations skip cell overwrite and emit
no new native glyphs. Resize invalidates the scene. Synchronized output and saved/restored cursor
state cover the full frame; failure exits through owned terminal restoration.
