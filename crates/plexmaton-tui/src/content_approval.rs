//! Logical lines for queued requests and the approval decision card.

use plexmaton_core::{ApprovalDecision, AttentionKind, ToolCapability};
use ratatui::text::{Line, Span};

use crate::{
    ViewState,
    theme::{Palette, Role},
};

/// Queued background requests, oldest first, with the cursor on the one `Enter` would go to.
///
/// Approval and clarification are drawn apart because `ui-ux.md` §attention management refuses one
/// generic notification treatment: one is an agent that cannot proceed, the other is an agent that
/// can. Seen requests stay listed and stop shouting — acknowledging is not resolving (ATT-3).
pub(crate) fn attention(state: &ViewState, palette: &Palette) -> Vec<Line<'static>> {
    let cursor = state.attention_cursor();
    state
        .attention()
        .enumerate()
        .map(|(index, item)| {
            let (marker, role) = match (item.acknowledged, item.kind()) {
                (true, _) => ("seen  ", Role::Muted),
                (false, AttentionKind::Approval) => ("block ", Role::ActionRequired),
                (false, AttentionKind::Clarification) => ("ask   ", Role::NewInformation),
            };
            let (caret, caret_role) = if index == cursor {
                ("> ", Role::Accent)
            } else {
                ("  ", Role::Muted)
            };
            Line::from(vec![
                Span::styled(caret, palette.style(caret_role)),
                Span::styled(marker, palette.style(role)),
                Span::styled(format!("{} · ", item.agent_id), palette.style(Role::Muted)),
                Span::styled(item.summary().to_owned(), palette.style(Role::Body)),
            ])
        })
        .collect()
}

/// The bounded decision card opened from an approval item in Attention.
pub(crate) fn approval(state: &ViewState, palette: &Palette, compact: bool) -> Vec<Line<'static>> {
    let Some(approval) = state.approval() else {
        return Vec::new();
    };
    let capabilities = approval
        .capabilities
        .iter()
        .map(|capability| match capability {
            ToolCapability::FileRead => "read files",
            ToolCapability::FileWrite => "change files",
            ToolCapability::ProcessSpawn => "run processes",
        })
        .collect::<Vec<_>>()
        .join(", ");
    let option = |decision, label| {
        let selected = approval.selected == decision;
        Line::from(vec![
            Span::styled(
                if selected { "> " } else { "  " },
                palette.style(if selected { Role::Accent } else { Role::Muted }),
            ),
            Span::styled(
                label,
                palette.style(if selected {
                    Role::ActionRequired
                } else {
                    Role::Body
                }),
            ),
        ])
    };

    let agent = Line::from(vec![
        Span::styled("Agent  ", palette.style(Role::Muted)),
        Span::styled(approval.agent_id.to_string(), palette.style(Role::Body)),
    ]);
    let mut tool = vec![
        Span::styled("Tool   ", palette.style(Role::Muted)),
        Span::styled(approval.tool.to_owned(), palette.style(Role::Body)),
    ];
    if !compact {
        tool.push(Span::styled(
            format!(" · {}", approval.call_id),
            palette.style(Role::Muted),
        ));
    }
    let tool = Line::from(tool);
    let access = Line::from(vec![
        Span::styled("Access ", palette.style(Role::Muted)),
        Span::styled(capabilities, palette.style(Role::ActionRequired)),
    ]);
    let detail = Line::styled(approval.detail.to_owned(), palette.style(Role::Body));

    if compact {
        return vec![
            agent,
            tool,
            access,
            option(ApprovalDecision::AllowOnce, "Allow once"),
            option(ApprovalDecision::Deny, "Deny"),
            Line::styled("↑↓ choose · Enter · PgDn · Esc", palette.style(Role::Muted)),
            Line::styled("Details", palette.style(Role::SectionHeading)),
            detail,
        ];
    }

    vec![
        agent,
        tool,
        access,
        Line::default(),
        option(ApprovalDecision::AllowOnce, "Allow once"),
        option(ApprovalDecision::Deny, "Deny"),
        Line::styled("↑↓ choose · Enter decide", palette.style(Role::Muted)),
        Line::styled(
            "PgUp/PgDn details · Esc keeps pending",
            palette.style(Role::Muted),
        ),
        Line::default(),
        Line::styled("Details", palette.style(Role::SectionHeading)),
        detail,
    ]
}
