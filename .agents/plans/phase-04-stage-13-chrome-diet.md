# Plan — Phase 04 stage 13: chrome diet

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) |
| Contract | [UI/UX](../ui-ux.md) §input, §surface model, §responsive layout classes |
| Status | Slice 1 of 4 done |

## Outcome

The primary conversation column has no box. The transcript runs into the composer's top rule and
ends with its activity line; the composer sits between two rules that carry only the addressee
and the reasoning effort; menus are a titled section above the top rule. The one-cursor rule,
the addressing rule and every surface's ownership are untouched: this stage moves lines, not
state. Decided by the user on 2026-09-06 from hand-composed frames at 95 columns.

## Slices

1. **Contract and stage record — done.** The §input rules for the two-rule composer and the
   activity line; the stage and roadmap rows; stage 11's plan waits on slice 4 here.
2. **The column without a box.** The transcript region drops its borders and keeps a two-cell
   inset; its last row is the activity line, drawn from the same derivation that fed the divider
   label (COM-5), so `Thinking`, `Responding`, `Running <tool>` and `Approval required` move
   there and the composer's rules never carry them. `specs/composer.md` §current work and
   `specs/transcript-layout.md` say where the row lives and what it costs the tail. Closed by
   a test that the label is on the transcript's last row and absent from both rules in every
   state, the tail-follow tests re-run against a one-row activity line, and idle, thinking, tool
   and approval frames at wide, medium and narrow.
3. **The two-rule composer.** The divider and the box's bottom edge become two rules. The top
   rule names the target and the reasoning effort, supplied by the composition root from the
   resolved model, as `ConfigurationSummary` already is; the bottom rule closes the input. The
   height cap rises from three lines to a third of the column, and past it `↑`/`↓` and the
   wheel move through the draft with the window following the caret. The collapsed row becomes
   one rule reading `Message Agent A · ⇥ to return`. `specs/composer.md` §place and §height are
   rewritten. Closed by COM-1's caret proofs re-run on the new geometry, a growth-and-cap test at
   three widths, a wheel-through-the-draft test, and empty, grown and windowed frames.
4. **Menus on the new chrome.** The Skills menu, and the Commands menu stage 11 adds, are a
   titled rule, rows and a muted key line above the top rule, no box; SKP-4 is rewritten for
   that geometry. Closed by the skill-picker frames re-cut and the SKP proofs re-run.

## Order and why

1 first because the others are written in its terms. 2 before 3 because the activity line must
have a home before the divider stops carrying it. 4 last, and stage 11's menu slice after it, so
the menu is drawn once.

## Deliberately not in this plan

The Drawer's own edges, which mark a surface pulled over the column and stay until the user has
seen them beside the bare column; the approval card's chrome; the status line, which the user
owns; the agent rail and the Attention strip, open in the contract; and any motion.
