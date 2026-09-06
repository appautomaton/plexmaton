//! Drawn permission choices and their pointer rows share one bounded layout.
use crate::{
    state::{
        permissions::{PermissionChoice, PermissionPanel},
        wrap_line,
    },
    theme::{Palette, Role},
};
use ratatui::text::{Line, Span};

pub(crate) struct PermissionContent {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) choices: Vec<(usize, PermissionChoice)>,
}

pub(crate) fn content(
    panel: &PermissionPanel,
    palette: &Palette,
    width: u16,
    height: u16,
) -> PermissionContent {
    let insets = crate::surface::ContentInsets::for_surface(crate::SurfaceId::Drawer, height);
    let budget = usize::from(height.saturating_sub(2 + 2 * insets.vertical));
    let choices = panel.choices();
    let count = choices.len().min(6).min(budget.saturating_sub(1));
    let start = panel.selected().saturating_sub(count.saturating_sub(1));
    let extras = if budget >= count + 4 { 3 } else { 0 };
    let mut heading: Vec<_> = panel
        .description()
        .iter()
        .flat_map(|line| wrap_line(line, usize::from(width)))
        .collect();
    let heading_budget = budget.saturating_sub(count + extras);
    if heading.len() > heading_budget {
        heading.truncate(heading_budget);
        if let Some(last) = heading.last_mut() {
            last.push('…');
        }
    }
    let mut lines: Vec<_> = heading
        .into_iter()
        .map(|line| {
            Line::styled(
                crate::content::command_summary(&line, usize::from(width)),
                palette.style(Role::Muted),
            )
        })
        .collect();
    if extras > 0 {
        lines.push(Line::default());
    }
    let mut positions = Vec::new();
    for (index, (choice, label)) in choices.into_iter().enumerate().skip(start).take(count) {
        positions.push((lines.len(), choice));
        let selected = index == panel.selected();
        let role = if selected { Role::Accent } else { Role::Body };
        lines.push(Line::from(vec![
            Span::styled(if selected { "> " } else { "  " }, palette.style(role)),
            Span::styled(
                crate::content::command_summary(&label, usize::from(width.saturating_sub(2))),
                palette.style(role),
            ),
        ]));
    }
    if extras > 0 {
        lines.push(Line::default());
        lines.push(Line::styled(panel.hint(), palette.style(Role::Muted)));
    }
    PermissionContent {
        lines,
        choices: positions,
    }
}

/// Local feedback follows its call and contributes no semantic source or copied content.
pub(crate) fn saved_permission_lines(palette: &Palette) -> Vec<Line<'static>> {
    vec![
        Line::styled(
            "Warning · Project permission saved; tool did not run.",
            palette.style(Role::ActionRequired),
        ),
        Line::styled(
            "Review it under Ctrl-P · Permissions.",
            palette.style(Role::Muted),
        ),
        Line::default(),
    ]
}
