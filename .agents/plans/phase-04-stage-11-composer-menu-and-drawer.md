# Plan — Phase 04 stage 11: composer menu and Drawer

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) |
| Contract | [UI/UX](../ui-ux.md) §product vocabulary, §input, §surface model (Drawer), §responsive layout classes |
| Status | Slices 1 and 2 of 5 done |

## Outcome

Commands live where their addressee is. A `/` in the composer offers only what the conversation
it names can do to itself, and runs it against a target captured at that moment. Everything wider
than a conversation, the resolved configuration, saved conversations and permissions, is a page of
the Drawer, the workspace's own surface, pulled from the top edge by `Ctrl-P` from any focus state
without touching a draft. The detached command palette, its slash aliases and the palette hint are
gone, and the surface count does not grow: pages and menu rows are content.

## Slices

1. **Contract and stage record — done.** Vocabulary rows for Command, Skill, Composer menu, Drawer
   and Page; the composer completion rule; the closed-categories rule and the Drawer's geometry;
   the phase and roadmap rows. Closed by `check-citations.sh` passing and the budget report.
2. **Drawer — done.** Rename `CommandPalette` to `Drawer` in surface id, kind, state, render and router;
   dock it to the top edge at full width with height from content; make Configuration a page
   beside Conversations and Permissions, so `return_palette` and the separate configuration
   surface disappear; put New conversation first in Conversations; drop the filter's leading-slash
   tolerance; delete `StatusNote::CommandHint`, which offered the palette for a `/`.
   `Outcome.command` becomes `Outcome.page` for the composition root. Write
   `specs/drawer.md` under a `DRW` prefix, absorbing INV-11's discovery clauses and INV-12/INV-13;
   rename `specs/session-picker.md` to `specs/conversation-picker.md` with SPK numbers unchanged.
   Closed by a width sweep proving full width and a fixed top edge down to 48 × 12, the Escape
   ladder page → list → origin, the Drawer opened over a waiting approval staying above it with
   the card unchanged, a role test that the chosen row carries accent, one full composition per
   layout class for the page list with pages as cropped region fixtures, and the README's key
   table. `smoke-tui.py` loses its palette and skill walkthroughs, which component tests own, and
   keeps one `Ctrl-P` open-and-close probe beside its terminal-lifecycle checks.
3. **Manual compaction.** A `CompactionTrigger::Requested` path in the runtime that plans and
   publishes a checkpoint with no model-call continuation, admitted only while idle: no turn, no
   pending approval, no active compaction. Refusal is a typed value naming its reason. CPL-9 in
   `specs/compaction.md`. Closed by runtime tests for the idle path, each refusal, and
   interrupt and shutdown during a requested compaction, none dispatching a model step.
4. **Composer menu and `/compact`.** Rename `SkillPicker` to `ComposerMenu`; one `MenuIntent`
   replaces `CommandPaletteIntent` and `SkillPickerIntent`, and the router keys its menu grammar
   on `SurfaceKind`, not on the surface's id. `Command` returns as the conversation-command enum,
   `Compact` first, carrying a `CommandTarget` of agent, conversation, head and revision that the
   composition root revalidates before dispatch; a stale or busy target is a notice in that
   conversation, never a redirect. A menu accept outranks retry
   submission; a bare `Enter` with no menu keeps today's retry precedence. `specs/skill-picker.md`
   becomes `specs/composer-menu.md`, SKP numbers kept and a `CMD` prefix for command rows. Closed
   by tests for `/` listing only conversation commands, `/config` and `/compact please` staying
   literal, Tab versus
   Enter, Escape keeping the draft, the stale-target refusal, Unicode and paste inside the token,
   three `composer-menu-*` frames, and one runtime test driving `/compact` against a loopback
   fixture; the PTY smoke does not.
5. **Pressed-pointer consolidation.** One `Pressed { surface, target, at }` replaces the five
   `pressed_*` slots; one exhaustive `hit` per surface and one `activate` replace the chained
   pointer handlers, and the wildcard arms over `SurfaceId` go. Closed by the existing button tests
   plus one proving a press on one surface cannot activate a release on another, and one proving
   a press, a drag inside the same row and a release activate nothing.

## Order and why

1 first because every later slice is written in its vocabulary. 2 before 4 because the pages must
be reachable from the Drawer before the composer stops dispatching them. 3 before 4 so the Commands
menu lands with one real row and one real refusal, never an empty menu. 5 last because it
refactors the two surfaces that 2 and 4 shape.

## Deliberately not in this plan

Worker inputs offering `/` or `$`; targets other than the primary agent; `/branch` and `/rewind`,
which are Phase 02 stage 2's; editing configuration in the Drawer; Drawer motion, which is stage
10's clock if it is ever wanted; scope labels on menu rows, because placement is the scope; and any
hand-off from a typed `/config` to the Drawer, which would blur the line this stage draws.
