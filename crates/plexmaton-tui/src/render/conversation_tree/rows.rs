//! Message rows keep disclosure, identity, content and selection styles independent.
use super::{single_line, truncate};
use crate::{Palette, state::ConversationTree, theme::Role};
use plexmaton_core::{ConversationEntryId, HeadName, TreeRow, TreeRowKind};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(super) fn entry_line(
    tree: &ConversationTree,
    row: &TreeRow,
    width: u16,
    selected: bool,
    palette: &Palette,
) -> Line<'static> {
    let prefix = tree.ancestry_prefix(&row.entry_id);
    let kind = kind_name(row.kind);
    let (badge, badge_width) = head_badge(tree, row, width / 2, selected, palette);
    let label = row.label.as_ref().map_or_else(String::new, |label| {
        format!(" · {}", single_line(label.as_str()))
    });
    let preview = single_line(&row.preview.text);
    let fold = fold_control(tree, &row.entry_id);
    let prefix_width = u16::try_from(UnicodeWidthStr::width(prefix)).unwrap_or(width);
    let available = width
        .saturating_sub(badge_width)
        .saturating_sub(prefix_width)
        .saturating_sub(4);
    let heading = truncate(&format!("{kind}{label}  "), available);
    let heading_width =
        u16::try_from(UnicodeWidthStr::width(heading.as_str())).unwrap_or(available);
    let preview_width = available.saturating_sub(heading_width);
    let preview = truncate(&preview, preview_width);
    let padding =
        usize::from(preview_width).saturating_sub(UnicodeWidthStr::width(preview.as_str()));
    let body_role = if row.active_ancestry {
        Role::Body
    } else {
        Role::Muted
    };
    let mut spans = vec![
        Span::styled(prefix.to_owned(), row_style(palette, Role::Muted, selected)),
        Span::styled(
            format!("{fold} "),
            row_style(palette, Role::Accent, selected),
        ),
        Span::styled(
            heading,
            row_style(
                palette,
                if selected { Role::Chosen } else { body_role },
                selected,
            ),
        ),
        Span::styled(
            format!("{preview}{}", " ".repeat(padding)),
            row_style(palette, body_role, selected),
        ),
    ];
    spans.extend(badge);
    Line::from(spans)
}

pub(super) fn continuation_line(
    tree: &ConversationTree,
    row: &TreeRow,
    width: u16,
    palette: &Palette,
) -> Line<'static> {
    let rails = tree.connector_rails(&row.entry_id);
    if !tree.is_folded(&row.entry_id) {
        let tail = if tree.has_children(&row.entry_id) {
            " │ "
        } else {
            ""
        };
        return Line::styled(
            truncate(&format!("{rails}{tail}"), width),
            palette.style(Role::Muted),
        );
    }
    let (count, mut heads) = tree.folded_contents(&row.entry_id);
    heads.sort_by(|left, right| {
        (left.as_str() != tree.active_head_name())
            .cmp(&(right.as_str() != tree.active_head_name()))
            .then_with(|| left.cmp(right))
    });
    let mut parts = vec![(
        format!(
            "{rails}    {count} {}",
            if count == 1 { "entry" } else { "entries" }
        ),
        Role::Muted,
    )];
    if !heads.is_empty() {
        parts.push((
            format!(
                " · {} {}: ",
                heads.len(),
                if heads.len() == 1 {
                    "branch"
                } else {
                    "branches"
                }
            ),
            Role::Muted,
        ));
        for (index, name) in heads.iter().enumerate() {
            if index > 0 {
                parts.push((" · ".to_owned(), Role::Muted));
            }
            let current = name.as_str() == tree.active_head_name();
            parts.push((
                format!(
                    "{}{}",
                    if current { "● " } else { "" },
                    single_line(name.as_str())
                ),
                if current { Role::Accent } else { Role::Muted },
            ));
        }
    }
    Line::from(styled_parts(parts, width, false, palette).0)
}

pub(super) fn head_line(
    tree: &ConversationTree,
    name: HeadName,
    target: Option<&ConversationEntryId>,
    width: u16,
    selected: bool,
    palette: &Palette,
) -> Line<'static> {
    let active = tree.active_head_name() == name.as_str();
    let semantic_target = tree
        .rows()
        .iter()
        .find(|row| row.head_markers.contains(&name));
    let summary = semantic_target.map_or_else(
        || {
            if target.is_none() {
                "empty root".to_owned()
            } else {
                "No message row".to_owned()
            }
        },
        |row| {
            format!(
                "{}: {}",
                kind_name(row.kind),
                single_line(&row.preview.text)
            )
        },
    );
    let text = format!(
        "{} {}{}  {summary}",
        if active { "●" } else { "○" },
        single_line(name.as_str()),
        if active { " · current" } else { "" },
    );
    Line::styled(
        truncate(&text, width),
        palette.style(if selected { Role::Chosen } else { Role::Body }),
    )
}

fn kind_name(kind: TreeRowKind) -> &'static str {
    match kind {
        TreeRowKind::User => "you",
        TreeRowKind::Steering => "steer",
        TreeRowKind::Assistant => "assistant",
        TreeRowKind::ToolBatch => "tools",
        TreeRowKind::Checkpoint => "checkpoint",
        TreeRowKind::Notice => "notice",
    }
}

fn fold_control(tree: &ConversationTree, id: &ConversationEntryId) -> &'static str {
    if !tree.has_children(id) {
        " • "
    } else if tree.is_folded(id) {
        "[+]"
    } else {
        "[−]"
    }
}

// Selection contributes its background; each component retains its semantic foreground/weight.
fn row_style(palette: &Palette, role: Role, selected: bool) -> ratatui::style::Style {
    let style = palette.style(role);
    if selected {
        ratatui::style::Style {
            bg: palette.style(Role::Chosen).bg,
            ..style
        }
    } else {
        style
    }
}

fn head_badge(
    tree: &ConversationTree,
    row: &TreeRow,
    width: u16,
    selected: bool,
    palette: &Palette,
) -> (Vec<Span<'static>>, u16) {
    let mut markers = tree.head_markers(&row.entry_id).iter().collect::<Vec<_>>();
    markers.sort_by_key(|name| (name.as_str() != tree.active_head_name(), name.as_str()));
    if markers.is_empty() {
        return (Vec::new(), 0);
    }
    let mut parts = vec![(" [".to_owned(), Role::Muted)];
    for (index, name) in markers.iter().enumerate() {
        if index > 0 {
            parts.push((" · ".to_owned(), Role::Muted));
        }
        let active = name.as_str() == tree.active_head_name();
        parts.push((
            format!(
                "{}{}",
                if active { "● " } else { "" },
                single_line(name.as_str())
            ),
            if active { Role::Accent } else { Role::Muted },
        ));
    }
    parts.push(("]".to_owned(), Role::Muted));
    styled_parts(parts, width, selected, palette)
}

fn styled_parts(
    parts: Vec<(String, Role)>,
    width: u16,
    selected: bool,
    palette: &Palette,
) -> (Vec<Span<'static>>, u16) {
    let total = parts
        .iter()
        .map(|(text, _)| UnicodeWidthStr::width(text.as_str()))
        .sum::<usize>();
    let mut remaining = usize::from(width).saturating_sub(usize::from(total > usize::from(width)));
    let mut used = 0;
    let mut spans = Vec::new();
    for (text, role) in parts {
        let clipped = UnicodeWidthStr::width(text.as_str()) > remaining;
        let text = text
            .chars()
            .take_while(|ch| {
                let cells = UnicodeWidthChar::width(*ch).unwrap_or(0);
                if cells > remaining {
                    false
                } else {
                    remaining -= cells;
                    used += cells;
                    true
                }
            })
            .collect::<String>();
        spans.push(Span::styled(text, row_style(palette, role, selected)));
        if remaining == 0 || clipped {
            break;
        }
    }
    if total > usize::from(width) {
        spans.push(Span::styled("…", row_style(palette, Role::Muted, selected)));
        used += 1;
    }
    (spans, u16::try_from(used).unwrap_or(width))
}
