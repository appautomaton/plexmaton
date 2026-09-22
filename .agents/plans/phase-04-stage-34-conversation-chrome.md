# Plan — Phase 04 stage 34, conversation chrome

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) stage 34 |
| Contract | [ui-ux](../ui-ux.md) §transcript grammar and its activity-line rule; EFF-4 in [reasoning-effort](../specs/reasoning-effort.md); FR-1 in [frame-loop](../specs/frame-loop.md); TR-6 in [transcript-layout](../specs/transcript-layout.md); SEL-1 and SEL-2 in [selection-and-copy](../specs/selection-and-copy.md) |
| Evidence | [Conversation chrome spike](../spikes/conversation-chrome/README.md): the rendered candidates the user chose from and the survey of three other CLIs |
| Status | Slices 1 and 2 of 5 implemented and locally verified |

## Outcome

The user's turn is told by its surface rather than by a shout, and a working agent is told by a
row that moves and says only what is known: how long the step has run, at what effort, and how long
the route has been quiet. Both are presentation. Removing either changes no journal entry, no
request and no replay.

## Contract, inline until code cites it

**MOT-1 — One visible-only motion clock.** Every moving cell in the workspace wakes on the deadline
EFF-4 already owns. Absence of any visible moving cell disarms it; late wakes coalesce into the
current phase without queuing missed frames.

**MOT-2 — A mark frame is one cell.** Every glyph in a motion sequence has display width one under
the width rules the product already applies to its markers, and a test proves it for every
sequence, so a frame can never widen a row.

**MOT-3 — Motion changes presentation only.** A phase change invalidates the painted frame and never
the semantic revision (FR-1). Replaying a journal reproduces the same rows with the clock stopped.

## Slices

1. **User message band — implemented.** The user's rows sit on a band of lifted ground across the
   conversation's width, the action gutter included, opened by a `›` in the user's own blue,
   continuation indented under the text. The band is an eighteenth role, the palette's ground
   carried a third of the way to its line, so a theme still reaches every colour. Hover keeps its
   boundary rules and puts the copy action on the band. The accent bar is gone. Closed by frames at
   wide, medium and narrow, a hovered frame, SEL-2 copy unchanged, TR-6 spacing unchanged, and the
   user's message no longer drawing in the accent role.
2. **Composer rule names the model — implemented.** The user found `Message Plexmaton` said nothing:
   the primary composer can address only the primary agent, whose name the box above already
   carries. The rule now reads the model's display name and its effort, `Muse Spark 1.3 · high`, and
   plain `Message` before a model is named; a collapsed composer and a worker's input keep naming
   their agent, because there the name does distinguish. Closed by the renamed COM-4 test, refreshed
   frames, and every PTY smoke waiting on its own model's title.
3. **Motion owner.** The effort animation in `workspace/effort.rs` becomes the workspace's motion
   owner: one deadline, any number of visible users, the effort rail its first user with no change
   in behaviour. Closes with the existing effort tests passing unchanged and a test that nothing
   visible moving means no wake (MOT-1).
4. **Activity line.** The row opens with the mark's core moving, then the label, then muted `· 11s ·
   high effort`, and after five seconds without a model event `· quiet for 31s`. Elapsed reads
   `11s`, `2m 11s`, `1h 2m`. The mark wears blue, the server-tool colour while a provider-run search
   is the work, and stands still in the action-required colour while approval is required. The
   core's cycle is the outline morph the user chose in the spike; whether Unicode geometric shapes
   or the Nerd Font's Material Design outlines draw it, and at what length, is the user's call once
   their terminal font settles, recorded there. Closes with frames for thinking, responding, running
   tool, running search, approval, compacting and quiet at three widths; MOT-2 for the cycle; MOT-3
   as a test that a tick advances no semantic revision.
5. **Documents.** Evidence tables current; SVG review of the frames attached; MOT-1 to MOT-3
   promoted to a spec when code cites them; this plan deleted; the phase row says done.

## Order and why

The band first, because it needs no clock and its frames are the baseline every later frame is
compared against. The motion owner before the activity line, so the row is born on the shared clock
and never grows one of its own. Documents last, once the frames exist to review.

## Deliberately not in this plan

A token count on the row, because Responses reports usage only at the end of a turn. A verb list,
because the labels are semantic states. Folding long user messages. The mark itself, which is
[stage 10](./phase-04-stage-10-branding.md) and shares MOT-1 to MOT-3.
