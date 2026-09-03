//! Compact and disclosed presentation for one typed tool entry.

use plexmaton_core::{ToolCallStatus, ToolDetail};
use ratatui::text::{Line, Span};

use super::select_line;
use crate::{
    state::{EntryAppearance, ToolCallView},
    theme::{Palette, Role, tool_role},
};

pub(super) fn entry(
    tool: &ToolCallView,
    palette: &Palette,
    appearance: EntryAppearance,
) -> Vec<Line<'static>> {
    let compact = Line::from(vec![
        Span::styled(
            format!("{} ", marker(tool.status)),
            palette.style(tool_role(tool.status)),
        ),
        Span::styled(tool.label.clone(), palette.style(Role::Body)),
        Span::styled(
            format!(" · {}", status_label(tool.status)),
            palette.style(tool_role(tool.status)),
        ),
    ]);
    let compact = if appearance.hovered && !appearance.selected {
        Line::styled(compact.to_string(), palette.style(Role::Accent))
    } else {
        select_line(compact, palette, appearance.selected)
    };
    let mut lines = vec![compact];
    if !appearance.open {
        return lines;
    }
    if let Some(invocation) = &tool.presentation.invocation {
        append_detail(
            &mut lines,
            "invocation",
            invocation,
            palette,
            appearance.selected,
        );
    }
    if let Some(outcome) = &tool.presentation.outcome {
        append_detail(&mut lines, "outcome", outcome, palette, appearance.selected);
    }
    lines
}

fn append_detail(
    lines: &mut Vec<Line<'static>>,
    heading: &str,
    detail: &ToolDetail,
    palette: &Palette,
    selected: bool,
) {
    let omitted = match detail {
        ToolDetail::Text { omitted_bytes, .. } if *omitted_bytes > 0 => {
            format!(" · {omitted_bytes} bytes omitted")
        }
        ToolDetail::Text { .. } | ToolDetail::Diff { .. } => String::new(),
    };
    let heading = Line::styled(format!("  {heading}{omitted}"), palette.style(Role::Muted));
    lines.push(select_line(heading, palette, selected));
    let source = match detail {
        ToolDetail::Text { source, .. } => source,
        ToolDetail::Diff { patch } => patch,
    };
    lines.extend(source.split('\n').map(|row| {
        let line = Line::from(vec![
            Span::styled("  │ ", palette.style(Role::Muted)),
            Span::styled(row.to_owned(), palette.style(Role::Body)),
        ]);
        select_line(line, palette, selected)
    }));
}

/// Tool markers stay legible without colour so monochrome terminals keep the same status grammar.
const fn marker(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Queued => "[ ]",
        ToolCallStatus::AwaitingApproval => "[?]",
        ToolCallStatus::Running => "[~]",
        ToolCallStatus::Succeeded => "[+]",
        ToolCallStatus::Failed => "[!]",
        ToolCallStatus::Denied => "[x]",
        ToolCallStatus::Cancelled => "[-]",
    }
}

const fn status_label(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Queued => "queued",
        ToolCallStatus::AwaitingApproval => "approval required",
        ToolCallStatus::Running => "running",
        ToolCallStatus::Succeeded => "succeeded",
        ToolCallStatus::Failed => "failed",
        ToolCallStatus::Denied => "denied",
        ToolCallStatus::Cancelled => "cancelled",
    }
}

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        ToolCallId, ToolCallStatus, ToolDetail, ToolPresentation, TranscriptItemId,
    };

    use super::{entry, marker};
    use crate::{
        state::{EntryAppearance, ToolCallView},
        theme::Palette,
    };

    fn tool(status: ToolCallStatus, presentation: ToolPresentation) -> ToolCallView {
        ToolCallView {
            entry_id: TranscriptItemId::new("entry")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            id: ToolCallId::new("call").unwrap_or_else(|error| panic!("fixture: {error}")),
            label: "exec_command".to_owned(),
            status,
            presentation,
            revision: 0,
        }
    }

    /// ENT-2: every state uses one stable, monochrome-readable compact grammar.
    #[test]
    fn every_tool_status_is_one_named_logical_line() {
        let palette = Palette::monochrome();
        for (status, expected_marker, label) in [
            (ToolCallStatus::Queued, "[ ]", "queued"),
            (ToolCallStatus::AwaitingApproval, "[?]", "approval required"),
            (ToolCallStatus::Running, "[~]", "running"),
            (ToolCallStatus::Succeeded, "[+]", "succeeded"),
            (ToolCallStatus::Failed, "[!]", "failed"),
            (ToolCallStatus::Denied, "[x]", "denied"),
            (ToolCallStatus::Cancelled, "[-]", "cancelled"),
        ] {
            let lines = entry(
                &tool(status, ToolPresentation::default()),
                &palette,
                EntryAppearance::compact(false),
            );
            assert_eq!(lines.len(), 1, "{status:?} stopped being compact");
            let rendered = lines[0].to_string();
            assert!(
                rendered.contains(expected_marker),
                "{status:?}: {rendered:?}"
            );
            assert!(rendered.contains(label), "{status:?}: {rendered:?}");
            assert!(
                rendered.contains("exec_command"),
                "{status:?}: {rendered:?}"
            );
        }
    }

    /// ENT-4: disclosure is available for every lifecycle state that carries retained detail; the
    /// compact status remains first and omission is explicit without changing source.
    #[test]
    fn every_tool_status_can_disclose_the_same_typed_detail() {
        let palette = Palette::monochrome();
        let presentation = ToolPresentation {
            invocation: Some(ToolDetail::Text {
                source: "Command \"cargo test\"".to_owned(),
                omitted_bytes: 0,
            }),
            outcome: Some(ToolDetail::Text {
                source: "status: exited\nstdout:\nok".to_owned(),
                omitted_bytes: 17,
            }),
        };
        for status in [
            ToolCallStatus::Queued,
            ToolCallStatus::AwaitingApproval,
            ToolCallStatus::Running,
            ToolCallStatus::Succeeded,
            ToolCallStatus::Failed,
            ToolCallStatus::Denied,
            ToolCallStatus::Cancelled,
        ] {
            let lines = entry(
                &tool(status, presentation.clone()),
                &palette,
                EntryAppearance {
                    selected: false,
                    open: true,
                    hovered: false,
                },
            );
            let rendered = lines
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(rendered.starts_with(&format!("{} exec_command · ", marker(status))));
            assert!(rendered.contains("invocation\n  │ Command \"cargo test\""));
            assert!(rendered.contains("outcome · 17 bytes omitted"));
            assert!(rendered.contains("  │ stdout:\n  │ ok"));
        }
    }
}
