# Plan — Phase 04 stage 11: composer menu and Drawer

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) |
| Contract | [UI/UX](../ui-ux.md) §product vocabulary, §input, §surface model (Drawer), §responsive layout classes |
| Status | Slices 1 and 2 of 6 done; slice 2's Conversations page moves to the menu in slice 4 |

## Outcome

Commands are what the user does from inside a conversation, typed where they type. A `/` in the
composer offers `/new`, `/resume`, `/compact` and `/permissions` for the Session, and runs the
chosen one from the conversation the composer names against a target captured at that moment.
What outlives a Session, the resolved configuration and the Project and User permissions, is a
page of the Drawer, the workspace's own surface, pulled from the top edge by `Ctrl-P` from any
focus state without touching a draft. The detached command palette, `/config` and the palette
hint are gone, and the surface count does not grow: pages and menu rows are content.

Decided by the user on 2026-09-06. Slice 2 had put Conversations in the Drawer on an agent's
reading; the user's words placed them in the composer, so slice 4 moves them.

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
4. **Composer menu: `/compact`, `/new`, `/resume`.** Rename `SkillPicker` to `ComposerMenu`; one
   `MenuIntent` replaces `CommandPaletteIntent` and `SkillPickerIntent`, and the router keys its
   menu grammar on `SurfaceKind`, not on the surface's id. `Command` returns as the
   conversation-command enum carrying a `CommandTarget` of agent, conversation, head and revision
   that the composition root revalidates before dispatch; a stale or busy target is a notice in
   that conversation, never a redirect. `/new` accepts at once as `ConversationRequest::New`;
   `/resume` shows the saved conversations as menu rows, the text after it as the query, with
   loading and failure rows inside the menu, so the Drawer's Conversations page and `Page::Conversations`
   go and the listing, validation and replacement in the CLI stay as they are. A menu accept
   outranks retry submission; a bare `Enter` with no menu keeps today's retry precedence.
   `specs/skill-picker.md` becomes `specs/composer-menu.md`, SKP numbers kept and a `CMD` prefix
   for command rows; SPK-1's discovery clause moves with the rows. Closed by tests for `/`
   listing the four commands, `/config` and `/compact please` staying literal, Tab versus Enter,
   Escape keeping the draft, the stale-target refusal, `/resume` rows sharing SPK-1's identity
   and cancellation by keyboard and pointer, Unicode and paste inside the token, three
   `composer-menu-*` frames, and one runtime test driving `/compact` against a loopback fixture;
   the PTY smoke does not.
5. **Permissions by lifetime.** `/permissions` lists the Session's grants and the native
   file-change preset as menu rows; a row reviews and confirms in the menu through PER-7's
   revisioned intents. The Drawer's Permissions page keeps what outlives the Session: Project
   grants, project configuration trust under PER-8, and the User rules snapshot, read only.
   Closed by tests that a Session grant is not offered in the Drawer and a Project grant is not
   offered in the menu, revocation from each place reaching the one owner, and PER-7's frames
   re-cut per place.
6. **Pressed-pointer consolidation.** One `Pressed { surface, target, at }` replaces the five
   `pressed_*` slots; one exhaustive `hit` per surface and one `activate` replace the chained
   pointer handlers, and the wildcard arms over `SurfaceId` go. Closed by the existing button tests
   plus one proving a press on one surface cannot activate a release on another, and one proving
   a press, a drag inside the same row and a release activate nothing.

## Order and why

1 first because every later slice is written in its vocabulary. 3 before 4 so the Commands menu
lands with one real operation and one real refusal, never an empty menu. 4 before 5 because
`/permissions` is a menu row and needs the menu. 6 last because it refactors the surfaces that 2,
4 and 5 shape.

## Deliberately not in this plan

Worker inputs offering `/` or `$`; targets other than the primary agent; `/branch` and `/rewind`,
which are Phase 02 stage 2's; `/model`, which needs the runtime to change a conversation's model
mid-flight and is its own stage; editing configuration in the Drawer; Drawer motion, which is
stage 10's clock if it is ever wanted; scope labels on menu rows, because placement is the scope;
and any hand-off from a typed `/config` to the Drawer, which would blur the line this stage draws.
