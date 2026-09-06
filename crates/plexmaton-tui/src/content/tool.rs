//! Compact and disclosed presentation for one typed tool entry.

use crate::text_layout::paint::{Line, Span, Treatment};
use plexmaton_core::{ToolCallStatus, ToolDetail};

use crate::{
    state::{EntryAppearance, ToolCallView},
    theme::{Role, tool_role},
};

pub(super) fn prepared_entry(tool: &ToolCallView, appearance: EntryAppearance) -> Vec<Line> {
    let mut compact = Line::from(vec![
        Span::styled(format!("{} ", marker(tool.status)), tool_role(tool.status)),
        Span::styled(tool.label.clone(), Role::Body),
        Span::styled(
            format!(" · {}", status_label(tool.status)),
            tool_role(tool.status),
        ),
    ]);
    compact.treatment = Treatment::ToolHeading;
    let mut lines = vec![compact];
    if !appearance.open {
        return lines;
    }
    if let Some(invocation) = &tool.presentation.invocation {
        append_detail(&mut lines, "invocation", invocation);
    }
    if let Some(outcome) = &tool.presentation.outcome {
        append_detail(&mut lines, "outcome", outcome);
    }
    lines
}

fn append_detail(lines: &mut Vec<Line>, heading: &str, detail: &ToolDetail) {
    let omitted = match detail {
        ToolDetail::Text { omitted_bytes, .. } if *omitted_bytes > 0 => {
            format!(" · {omitted_bytes} bytes omitted")
        }
        ToolDetail::Text { .. } | ToolDetail::Diff { .. } => String::new(),
    };
    let heading = Line::styled(format!("  {heading}{omitted}"), Role::Muted);
    lines.push(heading);
    match detail {
        ToolDetail::Text { source, .. } => {
            append_source(lines, source, Treatment::Content, |_| Role::Body);
        }
        ToolDetail::Diff { patch } => {
            append_source(lines, patch, Treatment::Diff, diff_role);
        }
    }
}

fn append_source(
    lines: &mut Vec<Line>,
    source: &str,
    treatment: Treatment,
    role: impl Fn(&str) -> Role,
) {
    lines.extend(source.split('\n').map(|row| {
        let mut line = Line::from(vec![
            Span::styled("  │ ", Role::Muted),
            Span::styled(row.to_owned(), role(row)),
        ]);
        line.treatment = treatment;
        line
    }));
}

/// One bounded line-prefix decision, not a diff parser. Unknown and context lines remain source
/// text; decoration can never make the canonical patch invalid or expensive to understand.
fn diff_role(row: &str) -> Role {
    if row.starts_with("@@") {
        Role::Accent
    } else if row.starts_with("*** ") || row.starts_with("\\ No newline") {
        Role::Muted
    } else if row.starts_with('+') {
        Role::NewInformation
    } else if row.starts_with('-') {
        Role::Failure
    } else {
        Role::Body
    }
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

    use super::{marker, prepared_entry};
    use crate::{
        state::{EntryAppearance, ToolCallView},
        theme::{Palette, Role},
    };

    fn entry(
        tool: &ToolCallView,
        palette: &Palette,
        appearance: EntryAppearance,
    ) -> Vec<ratatui::text::Line<'static>> {
        let colors = crate::text_layout::paint::Colors::new(palette);
        prepared_entry(tool, appearance)
            .into_iter()
            .map(|line| line.paint_entry(&colors, appearance))
            .collect()
    }

    fn tool(status: ToolCallStatus, presentation: ToolPresentation) -> ToolCallView {
        ToolCallView {
            saved_project_permission: None,
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
                    copy_hovered: false,
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

    /// ENT-4: diff meaning comes from retained markers in monochrome and semantic roles in colour;
    /// file headers are metadata rather than false additions/removals.
    #[test]
    fn canonical_diff_lines_keep_markers_and_receive_bounded_semantic_roles() {
        let patch = "*** Begin Patch\n*** Update File: src/lib.rs\n@@ bytes 0..4; old_bytes=4; new_bytes=6 @@\n-blue\n+pastel\n---old flag\n+++new flag\n\\ No newline at end of edit\n*** End Patch";
        let presentation = ToolPresentation {
            invocation: None,
            outcome: Some(ToolDetail::Diff {
                patch: patch.to_owned(),
            }),
        };
        let appearance = EntryAppearance {
            open: true,
            ..EntryAppearance::default()
        };

        let monochrome = entry(
            &tool(ToolCallStatus::Succeeded, presentation.clone()),
            &Palette::monochrome(),
            appearance,
        );
        assert_eq!(monochrome[5].to_string(), "  │ -blue");
        assert_eq!(monochrome[6].to_string(), "  │ +pastel");
        assert_eq!(monochrome[7].to_string(), "  │ ---old flag");
        assert_eq!(monochrome[8].to_string(), "  │ +++new flag");

        let palette = Palette::pastel();
        let coloured = entry(
            &tool(ToolCallStatus::Succeeded, presentation),
            &palette,
            appearance,
        );
        for (line, role) in [
            (2, Role::Muted),
            (3, Role::Muted),
            (4, Role::Accent),
            (5, Role::Failure),
            (6, Role::NewInformation),
            (7, Role::Failure),
            (8, Role::NewInformation),
            (9, Role::Muted),
            (10, Role::Muted),
        ] {
            assert_eq!(coloured[line].spans[1].style, palette.style(role));
        }

        let selected = entry(
            &tool(
                ToolCallStatus::Succeeded,
                ToolPresentation {
                    invocation: None,
                    outcome: Some(ToolDetail::Diff {
                        patch: patch.to_owned(),
                    }),
                },
            ),
            &palette,
            EntryAppearance {
                selected: true,
                open: true,
                hovered: false,
                copy_hovered: false,
            },
        );
        assert_eq!(
            selected[6].spans[1].style,
            palette
                .style(Role::NewInformation)
                .patch(palette.style(Role::Selection)),
            "selection keeps the semantic diff role and adds its common treatment"
        );
    }

    /// ENT-4: decoration is one prefix check per logical line. A maximum-sized opaque line stays
    /// one exact body span rather than entering an unbounded parser or tokenizer.
    #[test]
    fn opaque_maximum_diff_line_degrades_to_exact_plain_text() {
        let source = format!(" {}", "x".repeat(64 * 1024 - 1));
        let lines = entry(
            &tool(
                ToolCallStatus::Succeeded,
                ToolPresentation {
                    invocation: None,
                    outcome: Some(ToolDetail::Diff {
                        patch: source.clone(),
                    }),
                },
            ),
            &Palette::pastel(),
            EntryAppearance {
                open: true,
                ..EntryAppearance::default()
            },
        );
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2].spans[1].content.as_ref(), source);
    }
}
