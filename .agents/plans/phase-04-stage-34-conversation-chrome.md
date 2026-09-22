# Plan — Phase 04 stage 34, conversation chrome

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) stage 34 |
| Contract | [ui-ux](../ui-ux.md) §transcript grammar and its activity-line rule; EFF-4 in [reasoning-effort](../specs/reasoning-effort.md); FR-1 in [frame-loop](../specs/frame-loop.md); TR-6 in [transcript-layout](../specs/transcript-layout.md); SEL-1 and SEL-2 in [selection-and-copy](../specs/selection-and-copy.md) |
| Evidence | [Conversation chrome spike](../spikes/conversation-chrome/README.md): the rendered candidates the user chose from and the survey of three other CLIs |
| Status | Slices 1–4 of 5 implemented and locally verified |

## Outcome

The user's turn is told by its surface rather than by a shout, and a working agent is told by a
row that moves and says only what is known: how long the step has run, at what effort, and how long
the route has been quiet. Both are presentation. Removing either changes no journal entry, no
request and no replay.

## Contract

MOT-1 to MOT-3 live in [motion](../specs/motion.md), promoted when the motion owner cited them.

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
3. **Motion owner — implemented.** The effort rail's clock became the workspace's one motion owner:
   one deadline, each moving thing answering only whether it is visible, the phase held at the top
   of view state where every renderer reads it. The effort rail is its first user with no change in
   behaviour. Closed by the effort tests passing unchanged and
   `mot_1_one_deadline_serves_every_mover_and_late_wakes_coalesce`.
4. **Activity line — implemented.** The row opens with the mark, then the label, then muted
   readings: elapsed since the current work began, the effort, and after five silent seconds `quiet
   for`. Elapsed reads `11s`, `2m 11s`, `1h 2m`; a short row drops effort, then quiet, then elapsed.
   The mark is the Nerd Font's circle, rounded square and square, outline then filled, eight frames
   at 200 ms, which the user chose in their own terminal over the Unicode cycles that shook. It
   wears blue, the server-tool colour while a hosted search runs, and stands still in the
   action-required colour during approval. The readings are presentation: events are dated as they
   are applied and the motion clock supplies the instant a frame is drawn for. Closed by refreshed
   current-work frames, the readings and work-clock tests, MOT-2 for the mark, and the activity
   smokes.
5. **Documents.** Evidence tables current; SVG review of the frames attached; this plan deleted;
   the phase row says done.

## Order and why

The band first, because it needs no clock and its frames are the baseline every later frame is
compared against. The motion owner before the activity line, so the row is born on the shared clock
and never grows one of its own. Documents last, once the frames exist to review.

## Deliberately not in this plan

A token count on the row, because Responses reports usage only at the end of a turn. A verb list,
because the labels are semantic states. Folding long user messages. The mark itself, which is
[stage 10](./phase-04-stage-10-branding.md) and shares MOT-1 to MOT-3.
