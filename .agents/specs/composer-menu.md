# Spec — Composer menu

| Field | Value |
| --- | --- |
| Status | Implemented; verified offline and at three rendered widths |
| Owns | What the primary composer's draft completes to: Skills for `$`, Commands for `/`, `/resume`'s saved conversations, `/permissions`' Session grants and model/effort choices; what leaves the workspace when a row is accepted |
| Depends on | SKL-2/SKL-4/SKL-5, COM-1/COM-3/COM-6, INV-1/INV-6, SURF-3; [conversation-picker](./conversation-picker.md) for `/resume`'s rows; CPL-9 for `/compact`; PER-7 for `/permissions` |
| Proven by | TUI, runtime and agent proofs below; real terminal completion smoke |

## Invariants

**SKP-1 — Discovery is a projection.** The primary composer consumes a bounded user-invocable
catalog supplied by the composition root; its menu performs no filesystem or runtime operation.
Names, descriptions and origin labels refer to the runtime's winning skill definitions (SKL-2).

**SKP-2 — Completion is an edit with identity.** Choosing a skill inserts `$name ` without sending
the message and binds only that exact initial token. Input handoff and failure retain the selected
name alongside original text; changing the token invalidates its binding (COM-3/COM-6).

**SKP-3 — The composer keeps input ownership.** The central interaction layer routes menu
navigation, acceptance, pointer selection and scrolling; ordinary typing retains the composer's
caret. Escape closes the menu without deleting the draft; worker inputs do not offer this menu.

**SKP-4 — The menu fits the conversation.** The menu is a titled rule and its rows above the
primary composer's top rule, bounded within its conversation column and available terminal
space. It clips summaries and scrolls choices without obscuring the input or taking another
conversation's space.

**CMC-1 — A Command runs from the conversation that typed it.** `/` lists the Commands; accepting
one leaves the workspace as a value the composition root runs: `/new` and `/resume` as a
`ConversationRequest`, `/compact` as a `CommandRun` whose target is the composer's agent, captured
at acceptance. The runtime admits or refuses the run; a refusal is one sentence after the
conversation's last entry (CPL-9), never a redirect to another conversation. Rejected: a target of
conversation, head and revision revalidated before dispatch, because acceptance and the run are one
loop step and the runtime's admission already names a busy conversation.

**CMC-2 — Only a whole Command runs.** `Tab` completes the chosen Command into the draft as
`/name ` and runs nothing; `Enter` runs a draft that is exactly a Command, with the menu open or
dismissed. A listing Command, `/resume`, `/permissions`, `/effort` or `/model`, keeps the text after it as its query.
A Command may also declare flags, which lead the text after the name: `/compact --force` is that
Command carrying that modifier, and a listing Command's query begins after them. A flag names how
the Command runs, never what it runs on; that is what a listing is for.
Any other draft with text after the token, `/compact please` and an undeclared `--flag` included,
is text and submits as text — a mistyped flag is visible as the message it became rather than
silently running the plain Command it was modifying. `/` followed by a character no Command starts with lists nothing. A whole Command is a
request, never unsent input a switch would lose (SPK-2).

**CMC-3 — Session permissions are typed where the Session is.** `/permissions` lists the
Session's grants and the native file-change preset as rows under the panel's description, asks
the retained owner for its view once as `PermissionRequest::Refresh`, and offers a row only while
the view is in. `Enter` on a row reviews it in the same menu with `Back` under the marker; `Enter`
on the confirmation leaves as `PermissionRequest::Change` carrying the reviewed revision, and the
menu waits on the owner's answer (PER-7). `Escape` returns one layer, review to rows, then closes
and withdraws the place. A draft that stops asking for the rows withdraws them unless a change is
with the owner. Project grants never appear here. In a short terminal the rows keep their room and
the description gives way, ending in `…`.

## Grammar

At the start of a primary draft, `$` opens available skills and `/` the Commands, `/new`,
`/resume`, `/compact`, `/permissions`, `/effort` and `/model`; subsequent characters filter. Up/Down select, Tab or
Enter completes a skill, Tab completes a Command and Enter accepts it, and Escape dismisses. A completed `$name request` submits normally on the next
Enter. Exact unselected nonnumeric skill names activate only when present in the user-invocable
catalog. Unknown variables, `$HOME`, currency, command substitutions and dollar expressions inside
prose/code remain literal text. Numeric skill names can be deliberately selected from the menu;
unbound `$100` remains currency. There is no `/skill:` execution alias.

[MDL-1–MDL-4](./model-selection.md) own configured model identity, filtering, acceptance and lifetime.

The menu shows up to five choices and keeps the selected row and controls visible when height is
constrained; it is suppressed if even one choice and the controls cannot fit. Source labels precede
truncatable descriptions. Displayed metadata is flattened to inert single-line text while its
semantic source remains unchanged. Editing within the initial token can complete without changing
the request suffix. Dismissal survives caret motion until that token changes.

Selected names accompany submitted, returned and retry-editor input. A historical numeric skill
selection comes from its typed journal activation, never from interpreting currency-shaped text.

## Evidence

[Named proofs](../evidence/composer-menu.md), one row an invariant.
