# Spec — Transcript layout

| Field | Value |
| --- | --- |
| Status | Implemented |
| Owns | How tall a conversation is, which part of it a frame builds, and where its reader is |
| Depends on | [surface-model](./surface-model.md) SURF-5 for retained state; the wheel rules in [interaction-routing](./interaction-routing.md) INV-3 |
| Proven by | `plexmaton-tui::transcript`, `::render`, and `::state::scroll` tests |

## Invariants

**TR-1 — A body is measured once per entry, revision, disclosure, feedback, and width.** An entry's body height comes from the
same wrapper that paints it (surface-model §viewports) and is recomputed only when that entry's
revision, open state, anchored restoration feedback/retry actions, or the panel's width changes: a delta re-measures one entry, disclosure
re-measures one entry at each retained width, a resize invalidates that width's heights, a tool lifecycle
update re-measures its one stable entry, and an unchanged frame re-measures none. Literal heights
use count-only wrapping; rich/disclosed entries keep explicit estimates until PRE-3 prepares them
when reached, then adopt that result once. MD-4's retained rows keep their own prepared revision
and height while newer source is pending; adopting a newer layout updates that height. Feedback
measurement and painting compose the same before/after rows. A palette change
invalidates neither prepared text nor height geometry (MD-4). Text, tool, artifact and mail entries use
the same ordered height cache. Markdown's separately bounded layout cache follows MD-4; evicting
prepared rows does not discard heights or anchors. TR-6 adds neighbour-dependent separators
without repeating body measurement. Width is part of the key,
because a conversation changes
width when the second window opens beside it at ultrawide and comes back; a conversation keeps one
set of heights per width, bounded at two, evicting the width least recently measured.

**TR-2 — A frame builds only what it draws.** The lines a frame constructs are bounded by the
entries its viewport reaches, not by the conversation's length; off-screen entries contribute their
measured or estimated height and nothing else. A reached open tool contributes prepared detail to
the same parent paragraph and scroll owner; exceeding PRE-1's preparation bound yields an explicit
refusal with ENT-4's complete retained source still copyable.
Building accumulates reached entry origins after one window-prefix calculation; each entry does
not repeat a whole-history prefix sum.

**TR-3 — A reading position is an entry, not a row.** A parked conversation is anchored to a
transcript entry and a row inside it, so the same semantic fact stays on screen across a resize and
content arriving elsewhere does not move it. A width the reader left and returned to paints the frame it
had: resolving an anchor clamps the row for display without writing the clamp back, and only a
scroll rewrites the stored position. Both directions of the row-to-entry conversion resolve at the
width that produced the viewport being scrolled, carried out of the frame on the viewport itself.
Rejected: a row offset, which survives a resize as a number while naming different text; a
per-surface position, which loses A's place on returning from B (TR-5).

**TR-4 — Following the tail is a state.** A viewport at its last row is following and stays at the
newest line as content arrives; scrolling away parks it and scrolling back to the end resumes it.
Deriving this from `offset == max_offset` loses it at the first delta. A conversation shorter than
its viewport is painted at the bottom of it, so a window floating over the top covers empty rows or
rows already read (ui-ux §shelf).

**TR-5 — A reading position belongs to the conversation.** Each agent's transcript keeps its own
position, so selecting another agent and returning restores where its reader was (SURF-5).

Disclosure is a reading action. Before an entry changes height, the viewport's current semantic top
anchor is parked and tail-follow is dropped; resolving that anchor after measurement keeps the
content the reader was on rather than jumping to the newly enlarged tail.

**TR-6 — Composition owns group separators.** Consecutive compact tools have no gap; a tool group
followed by text, or text followed by another entry, has one standard blank separator under
[ui-ux](../ui-ux.md#transcript-grammar). Text retains its closing separator at the tail and before
entry-local feedback; a tool's trailing feedback already supplies the group-closing separator.
Separators participate in measured height, windows, anchors and painted caret edges, but not in
prepared source; appending a neighbour updates spacing without rewrapping the old body.
Artifact/mail adjacency is unchanged. Rejected: padding reasoning source or inserting hover-only
rows, which gives measurement, scrolling and message actions different boundaries.

## Model

```text
ViewState (immutable to the renderer)        TranscriptMetrics (outlives the frame)
   TranscriptPosition per agent  ────────▶   offset_of ──▶ row
                                             window    ──▶ entries this frame builds
   wheel ◀──────────────────────────────────  anchor_at ◀── row
```

An anchor is `{ item, rows }`. The entry identity is the durable half; the row inside it is clamped
to that entry's height at the current width when resolved, so an entry that wrapped shorter is not
overshot. An anchor whose entry no longer exists resolves to the tail. The heights live in
`TranscriptMetrics`, held by the [frame loop](./frame-loop.md) and keyed by agent and width; the
width a surface's rows were measured at travels on `Viewport::content_width`. Semantic heights and
offsets use `usize`; at Ratatui's `u16` widget-scroll boundary, complete logical lines are removed
from the built prefix before the remaining scroll is narrowed.

| Change | Entries wrapped |
| --- | --- |
| A delta on one entry | 1 |
| A new entry | 1 |
| A tool lifecycle transition | 1 |
| Opening or closing one tool detail | 1 per retained width when that width is next drawn |
| Attaching restoration feedback | 1 anchored entry; no semantic revision or copy content changes |
| A resize | Literal heights once; rich/disclosed entries when reached |
| Reaching an unprepared rich entry | Its deferred height, once |
| Paint, palette, selection, or revisiting measured geometry | 0 |

Summing heights and checking each cached one is still valid visits every entry each frame; that is
arithmetic over a `Vec`, not wrapping, and `TranscriptMetrics::wrapped` and `::lines_built` are
what make the claim testable. What those walks cost is measured in [frame-loop](./frame-loop.md)
§cost.

Literal messages measure through the same borrowed row-break iterator that produces styled lines
and copy ranges. Counting allocates neither glyph/style vectors nor hidden-message layouts. Rich
Markdown and disclosed tools reuse PRE-3's asynchronous preparation for measurement and paint. All text entry kinds
retain semantic style intent in the same preparation path (MD-5); neither cache's identity includes
palette. A palette replacement requests one repaint
without changing projection, selection or anchors; assigning the same palette does nothing.

## Failure modes

| Situation | Response |
| --- | --- |
| Panel too narrow to wrap into | An entry measures zero rows rather than dividing by a zero width |
| A conversation with no entries | A placeholder, or its restoration confirmation; no semantic entry is invented. Restoration at an empty-history anchor stays before the first later entry |
| A conversation nothing has measured | No window and no anchor, so the wheel leaves it alone rather than parking it at a guess |
| An offset past the end of the content | An empty window. Drawing something arbitrary would hide the clamping error that produced it |
| Content taller than `u16::MAX` rows | Retain the full semantic offset and remove complete built-prefix lines until the terminal widget can express the remainder |
| An anchored entry that no longer exists | Resolves to the tail. Nothing removes an entry yet, so cache pruning has never run |

## Evidence

[Named proofs](../evidence/transcript-layout.md), one row an invariant.
