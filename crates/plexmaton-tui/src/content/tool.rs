//! Compact and disclosed presentation for one typed tool entry.

use crate::text_layout::paint::{Line, Span, Treatment};
use plexmaton_core::{ServerToolAction, ServerToolStatus, ToolCallStatus, ToolDetail};

use crate::{
    content::transcript_presentation::{Row, append_source},
    state::{EntryAppearance, ServerToolView, ToolCallView},
    theme::{Role, tool_role},
};

pub(super) fn prepared_entry(tool: &ToolCallView, appearance: EntryAppearance) -> Vec<Row> {
    let mut compact = Line::from(vec![
        Span::styled(format!("{} ", marker(tool.status)), tool_role(tool.status)),
        Span::styled(tool.label.clone(), Role::Body),
        Span::styled(
            format!(" · {}", status_label(tool.status)),
            tool_role(tool.status),
        ),
    ]);
    compact.treatment = Treatment::EntryHeading;
    let mut lines = vec![Row::heading(compact)];
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

/// The provider's call in the tool row's grammar, with colour saying who ran it.
///
/// Same marker column, same name, same `·`, same disclosure, so a reader scans one column for
/// everything the model reached for. The marker's colour is what differs: a call that ended well
/// wears the server-tool role rather than a lifecycle's, because there was no lifecycle here to
/// colour, and one that failed wears failure like any other (ui-ux §transcript grammar). After the
/// `·` is what the route reported and nothing it did not: the query, an opened page, or a pattern
/// and where it was sought. A search the route reported without a query is the name alone.
/// Rejected: a row family of its own, which gave up the marker column and the disclosure tools
/// already have; and the plain succeeded colour, which read as a tool this harness ran inside the
/// fence. The user chose the grammar and the colour on 2026-09-21 from rendered candidates.
pub(super) fn prepared_server_tool(view: &ServerToolView, appearance: EntryAppearance) -> Vec<Row> {
    let Some(call) = &view.call else {
        // Placed where the provider began the call, and still running: the tool row's word for
        // it, in the colour that says whose work it is.
        let running = ToolCallStatus::Running;
        let mut compact = Line::from(vec![
            Span::styled(format!("{} ", marker(running)), Role::ServerTool),
            Span::styled(view.tool.name().to_owned(), Role::Body),
            Span::styled(format!(" · {}", status_label(running)), Role::ServerTool),
        ]);
        compact.treatment = Treatment::EntryHeading;
        return vec![Row::heading(compact)];
    };
    let (status, role) = match call.status {
        ServerToolStatus::Completed => (ToolCallStatus::Succeeded, Role::ServerTool),
        ServerToolStatus::Failed => (ToolCallStatus::Failed, Role::Failure),
    };
    let mut spans = vec![
        Span::styled(format!("{} ", marker(status)), role),
        Span::styled(view.tool.name().to_owned(), Role::Body),
    ];
    match &call.action {
        ServerToolAction::Search { queries } if queries.is_empty() => {}
        ServerToolAction::Search { queries } => {
            spans.push(Span::styled(" · ", role));
            spans.push(Span::styled(queries.join(", "), Role::Body));
        }
        ServerToolAction::OpenPage { url } => {
            spans.push(Span::styled(" · ", role));
            spans.push(Span::styled("opened ", Role::Muted));
            spans.push(Span::styled(url.clone(), Role::Body));
        }
        ServerToolAction::FindInPage { url, pattern } => {
            spans.push(Span::styled(" · ", role));
            spans.push(Span::styled(format!("\"{pattern}\""), Role::Body));
            spans.push(Span::styled(" in ", Role::Muted));
            spans.push(Span::styled(url.clone(), Role::Body));
        }
    }
    if status == ToolCallStatus::Failed {
        spans.push(Span::styled(format!(" · {}", status_label(status)), role));
    }
    let mut compact = Line::from(spans);
    compact.treatment = Treatment::EntryHeading;
    let mut lines = vec![Row::heading(compact)];
    if appearance.open
        && let Some(source) = view.action_source()
    {
        lines.push(Row::heading(Line::styled(
            "  action".to_owned(),
            Role::Muted,
        )));
        append_source(&mut lines, &source, Treatment::Content, |_| Role::Body);
    }
    lines
}

fn append_detail(lines: &mut Vec<Row>, heading: &str, detail: &ToolDetail) {
    let omitted = match detail {
        ToolDetail::Text { omitted_bytes, .. } if *omitted_bytes > 0 => {
            format!(" · {omitted_bytes} bytes omitted")
        }
        ToolDetail::Text { .. } | ToolDetail::Diff { .. } | ToolDetail::Command(_) => String::new(),
    };
    lines.push(Row::heading(Line::styled(
        format!("  {heading}{omitted}"),
        Role::Muted,
    )));
    match detail {
        ToolDetail::Command(command) => {
            let text = super::command_transcript_source(
                &command.source,
                &command.workspace_root,
                command.timeout_ms,
            );
            append_source(lines, &text, Treatment::Content, |_| Role::Body);
        }
        ToolDetail::Text { source, .. } => {
            append_source(lines, source, Treatment::Content, |_| Role::Body);
        }
        ToolDetail::Diff { patch } => {
            append_source(lines, patch, Treatment::Diff, diff_role);
        }
    }
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

/// Markers occupy one fixed column, so a busy transcript can be scanned down its left edge
/// for the state that matters; colour then says how urgent the row it lands on is.
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
        ServerTool, ServerToolAction, ServerToolCall, ServerToolStatus, ToolCallId, ToolCallStatus,
        ToolDetail, ToolPresentation, TranscriptItemId,
    };

    use super::{marker, prepared_entry, prepared_server_tool};
    use crate::{
        state::{EntryAppearance, ServerToolView, ToolCallView},
        theme::{Palette, Role},
    };

    fn server_tool(action: ServerToolAction, status: ServerToolStatus) -> ServerToolView {
        ServerToolView {
            entry_id: TranscriptItemId::new("search")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            tool: ServerTool::WebSearch,
            call: Some(ServerToolCall {
                tool: ServerTool::WebSearch,
                action,
                status,
            }),
            revision: 0,
        }
    }

    /// ENT-2: a call the provider has begun is one running row in the tool grammar, in the colour
    /// that says whose work it is, with nothing to disclose yet.
    #[test]
    fn a_running_server_tool_row_is_named_and_wears_the_server_tool_colour() {
        let palette = Palette::pastel();
        let running = ServerToolView {
            entry_id: TranscriptItemId::new("search")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            tool: ServerTool::WebSearch,
            call: None,
            revision: 0,
        };
        let open = EntryAppearance {
            selected: false,
            open: true,
            hovered: false,
            copy_hovered: false,
        };
        assert_eq!(
            server_entry(&running, &palette, EntryAppearance::compact(false)),
            ["[~] web_search · running"]
        );
        assert_eq!(
            server_entry(&running, &palette, open),
            ["[~] web_search · running"]
        );
        assert_eq!(
            marker_colour(&running, &palette),
            palette.style(Role::ServerTool).fg
        );
        assert_eq!(running.action_source(), None);
    }

    fn server_entry(
        view: &ServerToolView,
        palette: &Palette,
        appearance: EntryAppearance,
    ) -> Vec<String> {
        let colors = crate::text_layout::paint::Colors::new(palette);
        prepared_server_tool(view, appearance)
            .into_iter()
            .map(|row| {
                row.flattened()
                    .paint_entry(&colors, appearance)
                    .to_string()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    fn marker_colour(view: &ServerToolView, palette: &Palette) -> Option<ratatui::style::Color> {
        let colors = crate::text_layout::paint::Colors::new(palette);
        let appearance = EntryAppearance::compact(false);
        let rows = prepared_server_tool(view, appearance);
        let line = rows
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("a compact row"))
            .flattened()
            .paint_entry(&colors, appearance);
        line.spans.first().and_then(|span| span.style.fg)
    }

    /// ui-ux §transcript grammar: the provider's call keeps the tool row's grammar and says what
    /// the route reported and nothing further; colour says who ran it, and failure stays failure.
    #[test]
    fn a_server_tool_row_keeps_the_tool_grammar_and_claims_only_what_the_route_reported() {
        let palette = Palette::pastel();
        let search = |queries: &[&str]| ServerToolAction::Search {
            queries: queries.iter().map(|query| (*query).to_owned()).collect(),
        };
        for (action, expected) in [
            (
                search(&["latest stable Rust release"]),
                "[+] web_search · latest stable Rust release",
            ),
            (search(&["a", "b"]), "[+] web_search · a, b"),
            (search(&[]), "[+] web_search"),
            (
                ServerToolAction::OpenPage {
                    url: "blog.rust-lang.org/releases/".to_owned(),
                },
                "[+] web_search · opened blog.rust-lang.org/releases/",
            ),
            (
                ServerToolAction::FindInPage {
                    url: "releases.rs/docs/1.98.1".to_owned(),
                    pattern: "1.98.1".to_owned(),
                },
                "[+] web_search · \"1.98.1\" in releases.rs/docs/1.98.1",
            ),
        ] {
            let view = server_tool(action, ServerToolStatus::Completed);
            let lines = server_entry(&view, &palette, EntryAppearance::compact(false));
            assert_eq!(lines, [expected]);
            assert_eq!(
                marker_colour(&view, &palette),
                palette.style(Role::ServerTool).fg,
                "{expected}: the marker wears the server-tool colour"
            );
        }
        let failed = server_tool(
            search(&["Rust nightly changelog"]),
            ServerToolStatus::Failed,
        );
        assert_eq!(
            server_entry(&failed, &palette, EntryAppearance::compact(false)),
            ["[!] web_search · Rust nightly changelog · failed"]
        );
        assert_eq!(
            marker_colour(&failed, &palette),
            palette.style(Role::Failure).fg,
            "a failed call wears failure, wherever it ran"
        );
        assert_ne!(
            palette.style(Role::ServerTool).fg,
            palette.style(Role::NewInformation).fg,
            "the provider's marker is not the local tool's green"
        );
    }

    /// ENT-4: what the route reported discloses beneath the row, one fact per line, and a search
    /// the route reported without a query has nothing to disclose.
    #[test]
    fn a_server_tool_row_discloses_what_the_route_reported() {
        let palette = Palette::pastel();
        let open = EntryAppearance {
            selected: false,
            open: true,
            hovered: false,
            copy_hovered: false,
        };
        let sought = server_tool(
            ServerToolAction::FindInPage {
                url: "releases.rs/docs/1.98.1".to_owned(),
                pattern: "1.98.1".to_owned(),
            },
            ServerToolStatus::Completed,
        );
        assert_eq!(
            server_entry(&sought, &palette, open),
            [
                "[+] web_search · \"1.98.1\" in releases.rs/docs/1.98.1",
                "  action",
                "  │ url: releases.rs/docs/1.98.1",
                "  │ pattern: 1.98.1",
            ]
        );
        let bare = server_tool(
            ServerToolAction::Search {
                queries: Vec::new(),
            },
            ServerToolStatus::Completed,
        );
        assert_eq!(server_entry(&bare, &palette, open), ["[+] web_search"]);
    }

    fn entry(
        tool: &ToolCallView,
        palette: &Palette,
        appearance: EntryAppearance,
    ) -> Vec<ratatui::text::Line<'static>> {
        let colors = crate::text_layout::paint::Colors::new(palette);
        prepared_entry(tool, appearance)
            .into_iter()
            .map(|row| row.flattened().paint_entry(&colors, appearance))
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

    /// ENT-2: every state uses one stable compact grammar in a fixed column.
    #[test]
    fn every_tool_status_is_one_named_logical_line() {
        let palette = Palette::pastel();
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
        let palette = Palette::pastel();
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

    /// ENT-4: diff meaning comes from retained markers in the text and semantic roles in colour;
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

        let markers = entry(
            &tool(ToolCallStatus::Succeeded, presentation.clone()),
            &Palette::pastel(),
            appearance,
        );
        assert_eq!(markers[5].to_string(), "  │ -blue");
        assert_eq!(markers[6].to_string(), "  │ +pastel");
        assert_eq!(markers[7].to_string(), "  │ ---old flag");
        assert_eq!(markers[8].to_string(), "  │ +++new flag");

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
