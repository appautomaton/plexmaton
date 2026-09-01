# Plan — Phase 00 closure corrections

| Field | Value |
| --- | --- |
| Phase | 00 — Experience Skeleton, closing the exit gate |
| Contract | [inspector](../specs/inspector.md) INS-1, [selection-and-copy](../specs/selection-and-copy.md) SEL-3/SEL-5, [frame-loop](../specs/frame-loop.md) FR-1, [composer](../specs/composer.md) COM-1 |
| Status | slice 1 of 6 |

## Outcome

The exit gate stops overclaiming. An external review of the delivered phase found six defects, all
reproduced here before being accepted; four of them are behaviours the workspace's own specs already
forbid, and one is the phase's hardest declared requirement never having been built. When the last
slice lands, the inspector holds the inspected agent's conversation rather than a duplicate of the
activity column, a selection cannot outlive the surface that showed it, the producer boundary
refuses what its contract says cannot happen, and the exit gate names what is actually met.

## Slices

### 1. A selection cannot outlive what shows it

`ViewState::selection` stores the agent whose entries it indexes, and nothing clears it when that
agent stops being the one on screen. `selected_in` then stops highlighting it — the surface looks
empty — while `copy()` still reads through the stored agent. Reproduced: select a message in A's
conversation, move the rail to B, press `Ctrl-Y`, and A's text reaches the clipboard while B's
conversation is what the user is looking at. That is SEL-3 and SEL-5 both broken, and it is the one
finding whose failure mode is invisible.

**Changes.** Moving the agent selection, and an unpinned inspector following it, drop any selection
that named the agent leaving the screen. Dropping rather than rebinding: an index into A's messages
means a different message in B's, so carrying it across is worse than losing it.

**Proves it.** `a_selection_does_not_survive_the_surface_changing_agents`, asserting both that
`copy()` returns nothing and that the revision advanced so the highlight is repainted away.

**Unblocks.** Slice 5, which gives the inspector a second conversation and therefore a second way
for a selection's agent to change under it.

### 2. The projection refuses what its contract says cannot happen

Two boundary claims are documented and unenforced.

`TranscriptItemFinalized` is specified as "will receive no further deltas", and `append_delta` never
consults `finalized`. A producer can append after finalizing and the text lands silently.
`finalize_item` will also finalize twice given a continuing revision.

Stable identities claim through `new()` that empty and whitespace-only are impossible, and derive
`Deserialize` straight onto the inner `String`. `""` decodes into an `AgentId`. Nothing deserializes
untrusted JSON in the binary today, which is exactly why this is worth fixing now rather than after
a real producer exists.

**Changes.** A new `ReduceError::ItemAlreadyFinalized`, refused the same way every other producer
defect is — one event dropped, one visible notice, the stream continues. `#[serde(try_from =
"String")]` on the identity macro so the validating constructor is the only way in.

**Proves it.** `a_delta_after_finalization_is_refused_and_the_text_does_not_land`,
`finalizing_twice_is_refused`, `an_identity_cannot_be_deserialized_past_its_constructor`.

### 3. A frame costs what changed

FR-1 says producer traffic that alters nothing visible costs no frame. `apply` calls `touch()` on
every accepted event, so a repeated `AgentStatusChanged` with the same status, or a
`ToolActivityChanged` repeating a tool's current label and status, forces a repaint. Reproduced: two
no-op events, two revisions. `ViewState::select` has the same defect at the near end of a
conversation — `Shift-↑` at the oldest entry clamps and still touches — while `move_selection`
directly above it gets this right and has a test asserting so.

**Changes.** The event arms that can be no-ops report whether they changed anything, and `apply`
touches on that rather than on acceptance. `select` compares the focus it computed against the one
it had.

**Proves it.** `a_repeated_status_or_tool_state_costs_no_frame`,
`extending_a_clamped_selection_costs_no_frame`.

### 4. The caret follows the draft the user can see

Every panel wraps (`Wrap { trim: false }`), and the composer measures itself in `'\n'` alone:
`requested_rows` counts logical lines, and `place_cursor` takes the last logical line's full display
width and clamps it to the panel's inside width. Reproduced at 60 columns with a 150-character
draft: the composer correctly paints the wrapped tail ending at column 34, and the caret sits at
column 59 — on the right border, 25 cells from the text. COM-1 says there is exactly one cursor; it
does not say it may be in the wrong place.

**Changes.** The composer measures in wrapped rows at the width it is being drawn at, and the caret
is placed at the end of the last *visible* row.

**Proves it.** `a_wrapped_draft_puts_the_caret_at_the_end_of_the_text_not_on_the_border`, driven
through the real render path so the assertion is about the frame and not about a helper.

### 5. The inspector shows the inspected agent's conversation

[`specs/inspector.md`](../specs/inspector.md) opens with "The workspace can show one agent's
conversation. The canonical journey needs it to show two." What was built shows the inspected
agent's tools, artifacts and mail — the same content the activity column already draws for the
selected agent, in a panel that builds every line every frame. So the phase's hardest requirement —
canonical step 5, "A and B stream concurrently into independent virtualized transcripts" — has never
run, and neither have the two workloads §responsiveness workloads declares for it: *a large hidden
transcript opened into an inspector* and *two visible independently scrolling transcripts*. The
`open inspector` budget row measures a detail panel opening, not a conversation.

The detail is not lost by this. The activity column follows the selection, so selecting B is how the
user reaches B's tools, artifacts and mail — which is what canonical step 10 asks for, and it is
already what happens.

**Changes.** The inspector's body becomes a window over the inspected agent's measured transcript,
through the same `TranscriptMetrics` the conversation uses — already keyed by agent, and already
holding a per-agent reading position, so two independent readers is plumbing rather than a
mechanism. Its rect reserves a bottom strip for the steer input while it holds focus, so INS-5 and
the cursor survive. Selection over `SurfaceId::Inspector` indexes transcript items rather than the
three detail lists.

**Proves it.** `two_conversations_scroll_independently_and_neither_moves_the_other`,
`an_inspected_conversation_keeps_its_own_reading_position_across_a_close_and_reopen`, and the
journey's step-5 assertion rewritten to compare two different agents' painted text rather than
asserting a word appears.

**Unblocks.** Slice 6's two missing workloads.

### 6. Re-measure, and correct the record

**Changes.** Two harness workloads: a large hidden transcript opened into an inspector, and two
visible transcripts scrolled alternately. `ui-ux.md`'s budget table gains their rows and the
`open inspector` row is re-measured against a conversation. Then the claims that are now wrong:
`README.md` says `Esc` exits, which stopped being true at D-031; the journey's step-4/5 comment
claims "two conversations are on screen, and they are different conversations" while both panels
were showing the same agent; and the exit gate says one declared workload was not run when three
were.

**Proves it.** The harness's own work-count assertions, and `./scripts/check-citations.sh`.

## Order and why

1 first: it is the most severe, the smallest, and slice 5 adds a second way to trigger it, so fixing
it afterwards would mean fixing it twice. 2, 3 and 4 are independent of each other and of 5 — they
are separate boundaries — and go before 5 only because they are cheap and 5 is not. 5 before 6
because 6 measures what 5 builds and corrects claims 5 changes.

## Not in this plan

The two O(n) budget rows read over target on a loaded machine and under it on a quiet one, and a
third independent measurement on someone else's machine reproduced the loaded reading. That is the
spread D-041 exists to make visible, not a regression: the harness asserts work counts, which are
identical on every machine. The item walk that causes it is already named in the Phase 01 handoff as
the first thing to look at, and optimizing it here would be optimizing without a user.
