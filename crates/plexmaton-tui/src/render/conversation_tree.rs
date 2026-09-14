//! The native conversation-tree panel (TRE-1/TRE-6).

use plexmaton_core::{ConversationEntryId, HeadName};
use ratatui::{layout::Rect, text::Line};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    chrome::title,
    panel::{Body, Chrome, Edges, Panel},
};
use crate::{
    Palette, ViewState, layout,
    state::{Caret, ConversationTree, TextInput, TreeEditor, TreeMode, TreePending},
    surface::Viewport,
    theme::Role,
};

mod rows;
use rows::{continuation_line, entry_line, head_line};

/// A hit in the tree, always returned as a stable identity rather than a row number (TRE-6).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Hit {
    Close,
    Fold(ConversationEntryId),
    Entry(ConversationEntryId),
    Head(HeadName),
}

/// Number of semantic entry/head rows available below the tree's fixed header and editor copy.
/// Workspace cursor movement calls the same calculation as painting and hit testing (TRE-6).
pub(crate) fn row_capacity(state: &ViewState, bounds: Rect) -> usize {
    let physical_rows = usize::from(bounds.height.saturating_sub(3));
    let static_rows = state.tree().map_or(1, static_rows);
    (physical_rows.saturating_sub(static_rows) / state.tree().map_or(1, row_stride)).max(1)
}

fn row_stride(tree: &ConversationTree) -> usize {
    if tree.mode() == TreeMode::Entries {
        2
    } else {
        1
    }
}

fn static_rows(tree: &ConversationTree) -> usize {
    if tree.unavailable().is_some() {
        return 1;
    }
    1 + usize::from(tree.notice().is_some()) + usize::from(tree.editor().is_some())
}

/// Caret position for the visible tree-local prompt, if one owns the workspace's cursor.
pub(crate) fn editor_caret(state: &ViewState, bounds: Rect) -> Option<Caret> {
    let tree = state.tree()?;
    let editor = tree.editor()?;
    let input = editor.input()?;
    let prefix = editor_prefix(editor);
    let width = bounds.width.saturating_sub(2);
    let prefix_width = u16::try_from(UnicodeWidthStr::width(prefix)).unwrap_or(u16::MAX);
    let input_width = width.saturating_sub(prefix_width).max(1);
    let input_caret = input.caret(input_width, 1);
    Some(Caret {
        row: u16::try_from(1 + usize::from(tree.notice().is_some())).unwrap_or(u16::MAX),
        column: prefix_width.saturating_add(input_caret.column),
    })
}

pub(super) fn panel(state: &ViewState, palette: &Palette, bounds: Rect) -> Panel {
    let width = bounds.width.saturating_sub(2);
    let physical_rows = usize::from(bounds.height.saturating_sub(3));
    let tree = state.tree();
    let mode = tree.map_or(TreeMode::Entries, ConversationTree::mode);
    let mut lines = Vec::new();
    let mut static_rows: usize;
    let mut tree_rows = 0_usize;
    let mut offset = 0_usize;

    if let Some(tree) = tree {
        if let Some(unavailable) = tree.unavailable() {
            lines.push(Line::styled(
                truncate(unavailable, width),
                palette.style(Role::Failure),
            ));
            static_rows = 1;
        } else {
            let header = match mode {
                TreeMode::Entries => format!(
                    "Messages · active branch {} · {}",
                    single_line(tree.active_head_name()),
                    branch_count(tree.heads_count())
                ),
                TreeMode::Heads => format!(
                    "Branches · current branch {} · Enter selects",
                    single_line(tree.active_head_name())
                ),
            };
            lines.push(Line::styled(
                truncate(&header, width),
                palette.style(Role::Muted),
            ));
            static_rows = 1;
            if let Some(notice) = tree.notice() {
                lines.push(Line::styled(
                    truncate(notice, width),
                    palette.style(Role::Failure),
                ));
                static_rows += 1;
            }
            if let Some(editor) = tree.editor() {
                let line = match editor {
                    TreeEditor::RenameHead { input, .. } => {
                        editor_input_line("Rename branch: ", input, width, palette)
                    }
                    TreeEditor::SetLabel { input, .. } => {
                        editor_input_line("Label message: ", input, width, palette)
                    }
                    TreeEditor::ConfirmAbandon(head) => Line::styled(
                        truncate(
                            &format!(
                                "Retire branch {}? Enter confirms · Esc cancels",
                                single_line(head.as_str())
                            ),
                            width,
                        ),
                        palette.style(Role::Failure),
                    ),
                };
                lines.push(line);
                static_rows += 1;
            }
            let capacity = row_capacity(state, bounds);
            match mode {
                TreeMode::Entries => {
                    let rows = tree.visible_entries();
                    tree_rows = rows.len();
                    if rows.is_empty() {
                        lines.push(Line::styled(
                            truncate("No saved messages yet · b shows branches", width),
                            palette.style(Role::Muted),
                        ));
                        static_rows = lines.len();
                    } else {
                        offset = visible_offset(tree, tree_rows, capacity);
                        let end = offset.saturating_add(capacity).min(rows.len());
                        for row in rows.iter().skip(offset).take(end.saturating_sub(offset)) {
                            let selected = tree.selected_entry() == Some(&row.entry_id);
                            lines.push(entry_line(tree, row, width, selected, palette));
                            lines.push(continuation_line(tree, row, width, palette));
                        }
                    }
                }
                TreeMode::Heads => {
                    let heads = tree.heads();
                    tree_rows = heads.len();
                    if heads.is_empty() {
                        lines.push(Line::styled(
                            truncate("No named branches are available.", width),
                            palette.style(Role::Muted),
                        ));
                        static_rows = lines.len();
                    } else {
                        offset = visible_offset(tree, tree_rows, capacity);
                        let end = offset.saturating_add(capacity).min(heads.len());
                        for head in heads.iter().skip(offset).take(end.saturating_sub(offset)) {
                            let selected = tree.selected_head() == Some(&head.name);
                            lines.push(head_line(
                                tree,
                                head.name.clone(),
                                head.target.as_ref(),
                                width,
                                selected,
                                palette,
                            ));
                        }
                    }
                }
            }
        }
    } else {
        lines.push(Line::styled(
            "Conversation tree is closed.",
            palette.style(Role::Muted),
        ));
        static_rows = 1;
    }

    let stride = tree.map_or(1, row_stride);
    // Fixed headings are painted with the window but do not scroll. Metadata describes only
    // complete node/connector pairs (or one-line branch rows), never a spare partial row.
    let content_rows = tree_rows.saturating_mul(stride).max(1);
    let capacity = physical_rows.saturating_sub(static_rows) / stride;
    let visible_rows = capacity.saturating_mul(stride).min(content_rows).max(1);
    let viewport = Viewport {
        content_rows,
        content_width: width,
        visible_rows: u16::try_from(visible_rows).unwrap_or(u16::MAX),
        offset: offset.saturating_mul(stride),
    };
    let footer = footer(tree, width, palette);
    Panel {
        insets: crate::surface::ContentInsets::default(),
        chrome: Chrome::Box,
        footer: Some(footer),
        body: Body::Window {
            lines,
            // The semantic row slice is already resolved above; only the viewport metadata uses
            // its absolute offset, so the fixed header remains painted while the cursor moves.
            skip_rows: 0,
            viewport,
        },
        title: title(palette, "Conversation tree", Role::SectionHeading, ""),
        badge: Some(Line::styled(" × ", palette.style(Role::Accent))),
        edges: Edges::All,
    }
}

fn footer(tree: Option<&ConversationTree>, width: u16, palette: &Palette) -> Line<'static> {
    let candidates: &[&str] = match tree {
        Some(tree) if tree.pending() == Some(TreePending::Navigation) => &[
            "Saving history… · Esc/× closes; write continues",
            "Saving… · Esc/× closes",
            "Esc/× closes",
        ],
        Some(tree) if tree.pending() == Some(TreePending::Edit) => &[
            "Saving tree details… · Esc/× closes; write continues",
            "Saving edit… · Esc/× closes",
            "Esc/× closes",
        ],
        Some(tree) if tree.unavailable().is_some() => {
            &["r refresh · Esc/× close", "r refresh · Esc/×", "Esc/×"]
        }
        Some(tree) if matches!(tree.editor(), Some(TreeEditor::ConfirmAbandon(_))) => &[
            "Enter retire · Esc cancel · × close",
            "Enter retire · Esc/× close",
            "Esc/× close",
        ],
        Some(tree) if tree.editor().is_some() => &[
            "Enter save · Esc cancel · × close",
            "Enter save · Esc/× close",
            "Esc/× close",
        ],
        Some(tree) if tree.notice().is_some() && tree.mode() == TreeMode::Heads => &[
            "↑↓ move · r refresh · b messages · Esc/× close",
            "r refresh · Esc/× close",
            "Esc/× close",
        ],
        Some(tree) if tree.notice().is_some() => &[
            "↑↓ move · r refresh · b branches · Esc/× close",
            "r refresh · Esc/× close",
            "Esc/× close",
        ],
        Some(tree) => match tree.mode() {
            TreeMode::Entries => return message_footer(tree, width, palette),
            TreeMode::Heads => &[
                "↑↓ move · Enter select · n rename · x retire · y copy · b messages · r refresh · Esc/× close",
                "↑↓ · ↵ select · n rename · x retire · b messages · Esc/× close",
                "↵ select · b messages · Esc/× close",
            ],
        },
        None => &["Esc close", "Esc"],
    };
    let message = candidates
        .iter()
        .copied()
        .find(|message| UnicodeWidthStr::width(*message) <= usize::from(width))
        .or_else(|| candidates.last().copied())
        .unwrap_or("Esc/×");
    Line::styled(truncate(message, width), palette.style(Role::Muted))
}

fn message_footer(tree: &ConversationTree, width: u16, palette: &Palette) -> Line<'static> {
    let row = tree.selected_entry_row();
    if row.is_none() {
        return Line::styled(
            truncate("No messages · b branches · Esc/× close", width),
            palette.style(Role::Muted),
        );
    }
    let action = match row {
        Some(row) if row.rewind == plexmaton_core::TreeRewindEligibility::Eligible => {
            "Enter rewind"
        }
        Some(_) => "Read-only",
        None => "No messages",
    };
    let fold = row
        .filter(|row| tree.has_children(&row.entry_id))
        .map_or("", |row| {
            if tree.is_folded(&row.entry_id) {
                " · f expand"
            } else {
                " · f collapse"
            }
        });
    let candidates = [
        format!(
            "↑↓ move · {action}{fold} · b branches · l label · y copy · r refresh · Esc/× close"
        ),
        format!("↑↓ · {action}{fold} · b branches · Esc/× close"),
        format!("{action} · b branches · Esc/× close"),
        "b branches · Esc/× close".to_owned(),
    ];
    let text = candidates
        .iter()
        .find(|text| UnicodeWidthStr::width(text.as_str()) <= usize::from(width))
        .unwrap_or(&candidates[3]);
    Line::styled(truncate(text, width), palette.style(Role::Muted))
}

fn branch_count(count: usize) -> String {
    if count == 1 {
        "1 branch".to_owned()
    } else {
        format!("{count} branches")
    }
}

fn editor_prefix(editor: &TreeEditor) -> &'static str {
    match editor {
        TreeEditor::RenameHead { .. } => "Rename branch: ",
        TreeEditor::SetLabel { .. } => "Label message: ",
        TreeEditor::ConfirmAbandon(_) => "",
    }
}

fn editor_input_line(
    prefix: &str,
    input: &TextInput,
    width: u16,
    palette: &Palette,
) -> Line<'static> {
    let prefix_width = u16::try_from(UnicodeWidthStr::width(prefix)).unwrap_or(u16::MAX);
    let input_width = width.saturating_sub(prefix_width).max(1);
    let value = input
        .visible_rows(input_width, 1)
        .into_iter()
        .next()
        .unwrap_or_default();
    Line::styled(
        truncate(&format!("{prefix}{value}"), width),
        palette.style(Role::Accent),
    )
}

fn visible_offset(tree: &ConversationTree, count: usize, visible_rows: usize) -> usize {
    let mut offset = tree.scroll_offset();
    if let Some(index) = tree.selected_index() {
        if index < offset {
            offset = index;
        } else if index >= offset.saturating_add(visible_rows) {
            offset = index.saturating_add(1).saturating_sub(visible_rows);
        }
    }
    offset.min(count.saturating_sub(visible_rows))
}

fn single_line(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn truncate(text: &str, width: u16) -> String {
    let width = usize::from(width);
    if UnicodeWidthChar::width(text.chars().next().unwrap_or(' ')).is_none() {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0_usize;
    let mut clipped = false;
    for ch in text.chars() {
        if ch.is_control() {
            continue;
        }
        let cell_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used.saturating_add(cell_width) > width {
            clipped = true;
            break;
        }
        result.push(ch);
        used = used.saturating_add(cell_width);
    }
    if clipped && width > 0 {
        while used >= width {
            if let Some(last) = result.pop() {
                used = used.saturating_sub(UnicodeWidthChar::width(last).unwrap_or(0));
            } else {
                break;
            }
        }
        result.push('…');
    }
    result
}

/// Resolves a row or close affordance against the exact cells painted by this panel.
pub(crate) fn hit(state: &ViewState, bounds: Rect, at: crate::surface::Point) -> Option<Hit> {
    let close = layout::conversation_tree_close_control(bounds);
    if at.x >= close.x && at.x < close.right() && at.y >= close.y && at.y < close.bottom() {
        return Some(Hit::Close);
    }
    if at.x <= bounds.x
        || at.x >= bounds.right().saturating_sub(1)
        || at.y <= bounds.y
        || at.y >= bounds.bottom().saturating_sub(2)
    {
        return None;
    }
    let tree = state.tree()?;
    if tree.editor().is_some() {
        return None;
    }
    let header = static_rows(tree);
    let local = usize::from(at.y.saturating_sub(bounds.y + 1));
    let item = local.checked_sub(header)?;
    match tree.mode() {
        TreeMode::Entries => {
            if item % row_stride(tree) != 0 {
                return None;
            }
            let item = item / row_stride(tree);
            if item >= row_capacity(state, bounds) {
                return None;
            }
            let rows = tree.visible_entries();
            let offset = visible_offset(tree, rows.len(), row_capacity(state, bounds));
            let row = rows.get(offset.saturating_add(item))?;
            if !tree.has_children(&row.entry_id) {
                return Some(Hit::Entry(row.entry_id.clone()));
            }
            let prefix = tree.ancestry_prefix(&row.entry_id);
            let fold_x = bounds
                .x
                .saturating_add(1)
                .saturating_add(u16::try_from(UnicodeWidthStr::width(prefix)).unwrap_or(u16::MAX));
            Some(if (fold_x..fold_x.saturating_add(3)).contains(&at.x) {
                Hit::Fold(row.entry_id.clone())
            } else {
                Hit::Entry(row.entry_id.clone())
            })
        }
        TreeMode::Heads => {
            let offset = visible_offset(tree, tree.heads().len(), row_capacity(state, bounds));
            tree.heads()
                .get(offset.saturating_add(item))
                .map(|head| Hit::Head(head.name.clone()))
        }
    }
}
