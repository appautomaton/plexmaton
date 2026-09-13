//! Bounded, pointer-addressable composer-menu presentation (SKP-4, CMC-1).

use super::{chosen_row, command_summary, inert_inline};
use crate::{Palette, Role, ViewState};
use ratatui::text::{Line, Span};

/// Bounded composer completions; every choice occupies exactly one pointer-addressable row.
/// The composer menu's rows, by the token the draft starts with, and its key line (SKP-4, CMC-1).
///
/// Skills carry their source and name; Commands their name and what `Enter` does; conversations
/// their title and, muted, their identity; permissions their label. The chosen row is the bar
/// (`Chosen`). The Conversations listing adds one status row while it has no rows or an open in
/// flight; the Session permissions listing opens with its panel's description as a heading.
pub(crate) fn composer_menu(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    use crate::state::MenuRow;
    if state.menu_listing() == Some(crate::Listing::Effort) {
        return crate::render::effort::lines(state, palette, width, height);
    }
    if state.menu_listing().is_none() {
        return Vec::new();
    }
    let menu = state.composer_menu();
    let input = state.composer();
    let status = state.menu_status();
    let rows = state.menu_rows();
    // The titled rule and the key line; the composer's top rule closes the menu (SKP-4). The
    // rows keep their room in a short terminal; the heading takes what is left and ends in `…`.
    let budget =
        usize::from(height.saturating_sub(2)).saturating_sub(usize::from(status.is_some()));
    let mut heading = state.menu_heading(width);
    let heading_budget = budget.saturating_sub(rows.len().min(crate::state::VISIBLE_ROWS));
    if heading.len() > heading_budget {
        heading.truncate(heading_budget);
        if let Some(last) = heading.last_mut() {
            last.push('…');
        }
    }
    let visible = budget.saturating_sub(heading.len());
    let window = menu.window(input.text(), input.cursor(), visible);
    let mut lines = Vec::with_capacity(window.len().saturating_add(2 + heading.len()));
    let failures = state.menu_heading_failure_rows(width);
    lines.extend(heading.into_iter().enumerate().map(|(index, line)| {
        Line::styled(
            command_summary(&format!("  {line}"), usize::from(width)),
            palette.style(if index < failures {
                Role::Failure
            } else {
                Role::Muted
            }),
        )
    }));
    for row in rows.iter().skip(window.start).take(window.len()) {
        let chosen = menu.chosen() == Some(row);
        let marker = if chosen { ">" } else { " " };
        let (name, detail) = match row {
            MenuRow::Skill(name) => {
                let choice = menu.skill(name);
                (
                    format!(
                        "{} · ${name}",
                        choice.map_or("", |choice| choice.source.label())
                    ),
                    choice.map_or_else(String::new, |choice| inert_inline(&choice.description)),
                )
            }
            MenuRow::Command(command) => {
                (format!("/{}", command.name()), command.summary().to_owned())
            }
            MenuRow::CommandAlias { command, name } => {
                (format!("/{name}"), command.summary().to_owned())
            }
            MenuRow::Conversation(id) => (
                menu.conversations
                    .as_ref()
                    .and_then(|picker| picker.choice(id))
                    .map_or_else(|| id.as_str().to_owned(), |choice| choice.title.clone()),
                id.as_str().to_owned(),
            ),
            MenuRow::Permission(choice) => (
                menu.permission_label(choice).unwrap_or_default(),
                String::new(),
            ),
            MenuRow::Model(identity) => (
                inert_inline(&format!(
                    "{}/{}{}",
                    identity.provider,
                    identity.model,
                    if state.is_current_model(identity) {
                        " · current"
                    } else {
                        ""
                    }
                )),
                menu.model_choice(identity)
                    .map_or_else(String::new, |choice| {
                        inert_inline(&format!("{} · {}", choice.display_name, choice.wire_id))
                    }),
            ),
            MenuRow::Effort(effort) => (effort.as_str().to_owned(), String::new()),
        };
        let text = if detail.is_empty() {
            command_summary(&format!("{marker} {name}"), usize::from(width))
        } else {
            command_summary(&format!("{marker} {name}  {detail}"), usize::from(width))
        };
        lines.push(if chosen {
            chosen_row(vec![Span::raw(text)], palette, width)
        } else {
            let mut split = text.len().min(marker.len() + 1 + name.len());
            while !text.is_char_boundary(split) {
                split -= 1;
            }
            Line::from(vec![
                Span::styled(text[..split].to_owned(), palette.style(Role::Body)),
                Span::styled(text[split..].to_owned(), palette.style(Role::Muted)),
            ])
        });
    }
    if let Some(status) = status {
        let role = if status.is_failure() {
            Role::Failure
        } else {
            Role::Muted
        };
        lines.push(Line::styled(
            command_summary(&format!("  {}", status.message()), usize::from(width)),
            palette.style(role),
        ));
    }
    lines.push(Line::styled(state.menu_keys(), palette.style(Role::Muted)));
    lines
}
