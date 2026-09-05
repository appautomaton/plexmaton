# Plan — Phase 04 stage 2, session interaction

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) |
| Contract | JRN-1–JRN-7, TIM-1–TIM-5, LOOP-6, TR-1–TR-4; user-approved retry and restoration interaction |
| Status | Active; slices 1–3 and message-action follow-up complete; slice 4 next |

## Outcome

Recover a conversation, retry an unanswered rate-limited request, edit it on a preserved branch,
or continue with a new user message. Mouse and keyboard use the same typed actions. Resume never
starts a model or tool. No tests contact a real model.

## Slices

1. **Restoration feedback.** Inline green confirmation after restored history, outside semantic
   entries and Notices. Explicit turn terminals prevent false interruption. Implemented, reviewed;
   three widths, scroll/copy and empty-history anchors proven.
2. **Retry and edit/retry.** Derive eligibility from acknowledged typed request outcomes and the
   current head. Only rate-limited turns with no model output/tools qualify. A retry execution
   references the original question without copying it into model input; old attempts remain.
   Edit/retry preserves the old branch and submits edited text from before the original question.
   *Closes when* both clicks and contextual keyboard actions work, duplicate/stale/busy actions do nothing,
   cancelled/failed persistence starts no effect and retains edited input, repeated retries reopen
   safely, and ignoring the actions produces two consecutive user atoms in both codecs.
   Implemented: offline workspace, HTTP/JSONL, pointer/keyboard and PTY tests passed. Backend and
   UI reviews resolved; cancellation, selection and focus fixes have regression tests. Inspected
   retry frames: [wide](../../crates/plexmaton-tui/frames/retry-wide.txt),
   [medium](../../crates/plexmaton-tui/frames/retry-medium.txt),
   [narrow](../../crates/plexmaton-tui/frames/retry-narrow.txt), plus command-palette frames.
3. **Session picker.** `/resume`, `/continue`, `/sessions`, `/session` find one command and one searchable
   history view. Listing is bounded and off the terminal loop; selection recovers through the
   existing session loader. Failed/locked/corrupt targets leave the current conversation intact.
   *Closes when* keyboard/mouse select the same session, no automatic model call occurs, and frames
   at wide/medium/narrow show empty, populated and failure states.
   Implemented under SPK-1–SPK-3; offline workspace tests and real PTY alias discovery passed.
   Reviewed [wide](../../crates/plexmaton-tui/frames/session-picker-wide.txt),
   [medium](../../crates/plexmaton-tui/frames/session-picker-medium.txt), and
   [narrow](../../crates/plexmaton-tui/frames/session-picker-narrow.txt) state panels;
   minimum-height selection, cancellation and locked-target regressions are component tests.
4. **Lazy automatic session creation.** Blank startup creates no JSONL; the first accepted message
   establishes persistence before dispatch. Explicit create/resume retain their defined meaning.
   *Closes when* blank exit leaves no file, first submission survives reopen, and failed creation
   returns the draft without a model call. Existing user files are not deleted.

## Message-action follow-up

User testing requested a hover-only Nerd Font Copy icon at each message's upper-right, separate
from text selection, plus lightweight Retry hover. SEL-7 owns this follow-up. Three-width frames
are `frames/message-actions-{wide,medium,narrow}.txt` in the TUI crate. The icon aligns with the
first content row; hover rules reuse blank separators without adding rows. TUI tests, Clippy,
offline terminal/status-line smoke and focused review passed. Character-level transcript
drag selection remains a separate outstanding change; current drags still span semantic entries.

## Boundaries

Normal composer submission appends new user input and invalidates old retry actions. Retry never
appends duplicate user text. Edit/retry owns its draft mode and preserves any displaced draft;
Escape cancels editing without changing the journal. Notices remains reserved for multi-agent
workflows. Markdown, general mid-tool retry, automatic backoff and compaction are outside this plan.
