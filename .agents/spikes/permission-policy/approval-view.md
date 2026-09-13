# Approval view experiment

| Field | Value |
| --- | --- |
| Read when | Implementing or reviewing the approval card and remembered-scope interaction |
| Status | User-reviewed padding and journey implemented; HTML retained as a finite visual fixture |
| Preview | [Open the standalone demo](./approval-view.html) |
| Basis | Current TUI at `b333364`; [approval-flow audit](./approval-flow.md); ATT-1–ATT-3, APV-4/APV-6 and P3–P5 |

## View

Keep the card inside the primary conversation, directly above its composer. Use the current
`Palette::pastel` roles, monospace cells, divider titles and vertical choices. The current runtime
has no worker rail until another agent exists, so the main-agent preview uses the whole conversation
width. The demo offers 120-, 95- and 60-column views at 24 rows. A View selector switches between
the approval card and a command-palette spacing proposal.

The collapsed card shows:

1. **What will happen:** a file path and change count, or the exact short command in the fixture.
2. **Why approval is needed:** the policy reason, separate from the assistant's conversational text.
3. **Choices:** Allow once, Allow and remember…, Deny. Deny starts selected. An explicit Ask rule
   offers only Allow once and Deny, following P3.
4. **Keys:** arrows/Enter, Ctrl-O for operation details and Esc back to the composer. Tab returns
   to the card. The draft remains available while a request waits.

The card's pending count belongs to this conversation. A later request must not replace the card
being answered. Background requests continue to use the explicit Attention entry point; this
mockup does not propose a new workspace-wide modal or implement a worker conversation.

## Internal spacing

The user requested more internal padding for approvals and identified the command palette as
crowded. The updated mockup applies the same proposed spacing to both:

- Two clear terminal cells inside each side border, measured before the selection marker.
- One blank row after the top divider and one before the bottom divider.
- One blank row between content, choices and key hints. Consecutive choices remain consecutive.

The approval composer stays at the same position as content wraps into the reduced width. The
palette keeps INV-13's three-cell outer margin, fixed top position and 76-column width cap; its
filter, command list and footer receive separate spacing. The existing `/config`, `/resume` and
`/new` rows keep one line each, truncating descriptions with an ellipsis where needed.

The production palette uses a shared content inset for rendering, input and geometry under INV-13.
PER-5/PER-10 own the approval card’s confirmed scope and short-terminal behavior. The HTML fixture
exercises only 24-row terminals; minimum-size evidence belongs to the Rust tests.

## Remembering

Allow and remember… opens a second step inside the same card. Entering this step grants nothing.
It names the workspace and backend-offered operation scope before offering a lifetime:

| Choice | Fixture meaning |
| --- | --- |
| This Session | In memory until Plexmaton exits; retained across `/new` and resume in the same workspace |
| This Project | Saved for this user's coding sessions in the checkout, including after restart |
| Back / Esc | Return without granting permission |

The user-approved Session/Conversation vocabulary is integrated into Rust, the UI and the contract.
The HTML fixtures retain an existing-file edit offer and an exact-command offer; they do not execute
permissions. Production uses PER-3’s native create/edit preset and PER-10’s backend-issued prefix or
exact offer, followed by This Session/This Project/Back in the same card. The
[prefix comparison](./command-prefixes.md) owns the parser findings. Scope availability and revision
validation are defined once in [permission policy](../../specs/permission-policy.md).

## Outcomes

Submitting replaces the actionable choices with an in-flight message. A duplicate Enter does
nothing. The demo's separate **Confirm result (demo)** control simulates producer confirmation;
it advances to a new request with Deny selected and preserves the draft. It is outside the proposed
terminal UI and performs no runtime action.

The state selector also exposes Permission changed, Save failed and Save uncertain. They show the
reason and a next action in the same region. An uncertain write offers reopen, not blind retry.
The reopening control only explains the proposed action; it cannot open or alter a real session.

Production submission and refusal behavior is implemented under PER-5/PER-6; the HTML result
selector remains a simulation of those visual states.

## Evidence and limits

The user opened the live demo and requested direct review without further screenshots. Keep that
preview available; screenshot capture is not required for this review. The initial wide view was
visually inspected. The updated padding passed 87 browser geometry cases across the three widths,
including approval states/details and palette filtering. Keyboard checks preserved the draft,
suppressed repeat submission and advanced only on simulated confirmation. The user reviews the
updated appearance directly; no additional screenshots were captured.

This self-contained HTML/SVG mockup reproduces the current TUI's visual vocabulary and cell sizes;
it is not output from Ratatui. It has no external resources, model calls, tool execution or saved
authority. Its event handlers are a finite presentation fixture, not the future permission backend.
The audit’s diagnostic frames describe the pinned base. [production evidence](./README.md#production-work)
records reviewed production frames and the real executable journey.
