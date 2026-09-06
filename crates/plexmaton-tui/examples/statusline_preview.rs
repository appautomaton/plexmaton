//! Design-only preview: existing workspace rendering plus proposed footer rows.
//! Run with an explicit output directory; no provider, user config or production layout is changed.

use std::{fmt::Write as _, path::Path};

use plexmaton_core::{
    AgentId, AgentStatus, ApprovalId, AttentionId, AttentionRequest, ConversationEvent,
    ConversationEventEnvelope, EventSequence, ToolCallId, ToolCallStatus, ToolCapability,
    ToolPresentation, TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{Palette, SurfaceId, Workspace};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SKY: Color = Color::Rgb(130, 180, 240);
const MINT: Color = Color::Rgb(140, 218, 165);
const VIOLET: Color = Color::Rgb(180, 150, 235);
const PINK: Color = Color::Rgb(235, 140, 200);
const TEAL: Color = Color::Rgb(120, 210, 205);
const LIME: Color = Color::Rgb(200, 224, 120);
const PLUM: Color = Color::Rgb(45, 40, 64);
const GOLD: Color = Color::Rgb(245, 208, 114);
const MUTED: Color = Color::Rgb(142, 162, 196);

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Ready,
    Quit,
    CommandHint,
    Approval,
}

fn main() -> Result<()> {
    let output = std::env::args()
        .nth(1)
        .ok_or("provide a preview output directory")?;
    let output = Path::new(&output);
    std::fs::create_dir_all(output)?;
    for (name, width) in [("wide", 120), ("medium", 95), ("narrow", 60)] {
        for (state, mode) in [
            ("ready", Mode::Ready),
            ("quit", Mode::Quit),
            ("command-hint", Mode::CommandHint),
            ("approval", Mode::Approval),
        ] {
            let buffer = preview(width, 26, mode)?;
            let stem = format!("{name}-{state}");
            std::fs::write(output.join(format!("{stem}.svg")), svg(&buffer))?;
            std::fs::write(output.join(format!("{stem}.txt")), plain(&buffer))?;
        }
    }
    Ok(())
}

fn preview(width: u16, height: u16, mode: Mode) -> Result<Buffer> {
    let footer = footer(width, mode);
    let footer_rows = u16::try_from(footer.len())?;
    // The old cwd row is replaced, so ask the existing layout for exactly the remaining space.
    let base_height = height - footer_rows + 1;
    let mut terminal = Terminal::new(TestBackend::new(width, base_height))?;
    let mut workspace = fixture(mode)?;
    workspace.draw(&mut terminal)?;
    if mode == Mode::Approval {
        for _ in 0..8 {
            if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Attention) {
                break;
            }
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
            workspace.draw(&mut terminal)?;
        }
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        workspace.draw(&mut terminal)?;
    }
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    for y in 0..base_height - 1 {
        for x in 0..width {
            buffer[(x, y)] = terminal.backend().buffer()[(x, y)].clone();
        }
    }
    Paragraph::new(footer).render(
        Rect::new(0, height - footer_rows, width, footer_rows),
        &mut buffer,
    );
    Ok(buffer)
}

fn fixture(mode: Mode) -> Result<Workspace> {
    let agent = AgentId::new("agent-primary")?;
    let mut events = vec![ConversationEvent::AgentCreated {
        agent_id: agent.clone(),
        label: "Plexmaton".into(),
        status: AgentStatus::Idle,
    }];
    for (id, role, text) in [
        (
            "user",
            TranscriptRole::User,
            "Can we inspect this repository and understand what we have?",
        ),
        (
            "answer",
            TranscriptRole::Assistant,
            "The journal owns session history. Provider adapters rebuild requests from that history. The status line will use a read-only snapshot of context, usage and cost.",
        ),
    ] {
        let item_id = TranscriptItemId::new(id)?;
        events.extend([
            ConversationEvent::TranscriptItemStarted {
                agent_id: agent.clone(),
                item_id: item_id.clone(),
                role,
            },
            ConversationEvent::TranscriptDelta {
                agent_id: agent.clone(),
                item_id: item_id.clone(),
                item_revision: 1,
                text: text.into(),
            },
            ConversationEvent::TranscriptItemFinalized {
                agent_id: agent.clone(),
                item_id,
                item_revision: 2,
            },
        ]);
    }
    if mode == Mode::Approval {
        let call_id = ToolCallId::new("fixture-call")?;
        for (revision, status) in [
            (0, ToolCallStatus::Queued),
            (1, ToolCallStatus::AwaitingApproval),
        ] {
            events.push(ConversationEvent::ToolCallChanged {
                agent_id: agent.clone(),
                item_id: TranscriptItemId::new("tool")?,
                item_revision: revision,
                call_id: call_id.clone(),
                label: "exec_command".into(),
                status,
                presentation: ToolPresentation::default(),
            });
        }
        events.push(ConversationEvent::AgentStatusChanged {
            agent_id: agent.clone(),
            status: AgentStatus::Waiting,
        });
        events.push(ConversationEvent::AttentionRequested { agent_id: agent.clone(), attention_id: AttentionId::new("fixture-attention")?,
            request: AttentionRequest::Approval { reason: plexmaton_core::ApprovalReason::PermissionRequired, remember: None, approval_id: ApprovalId::new("fixture-approval")?, call_id,
                tool: "exec_command".into(), capabilities: vec![ToolCapability::FileRead, ToolCapability::FileWrite, ToolCapability::ProcessSpawn],
                detail: "Command \"cargo test -p plexmaton-runtime\" · cwd \"/work/plexmaton\" · timeout 120000 ms".into() } });
    }
    let mut workspace = Workspace::with_palette(Palette::pastel());
    workspace.emit(
        events
            .into_iter()
            .enumerate()
            .map(|(index, event)| ConversationEventEnvelope {
                sequence: EventSequence::new(index as u64 + 1),
                event,
            })
            .collect(),
    );
    workspace.set_working_directory("~/dev/agents/coding/plexmaton".into());
    Ok(workspace)
}

fn piece(text: &str, color: Color) -> Span<'static> {
    Span::styled(text.to_owned(), Style::new().fg(color))
}

fn powerline(segments: &[(&str, Color)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (label, background)) in segments.iter().enumerate() {
        spans.push(Span::styled(
            format!(" {label} "),
            Style::new()
                .fg(PLUM)
                .bg(*background)
                .add_modifier(Modifier::BOLD),
        ));
        let next = segments
            .get(index + 1)
            .map_or(Color::Reset, |(_, color)| *color);
        spans.push(Span::styled(
            "\u{e0b0}",
            Style::new().fg(*background).bg(next),
        ));
    }
    Line::from(spans)
}

fn footer(width: u16, mode: Mode) -> Vec<Line<'static>> {
    let segments = [
        (" Luna  High", VIOLET),
        (" main", MINT),
        (" 28k/272k 10%", LIME),
        (" 82%", TEAL),
        (" ↑24.8k ↓3.2k", SKY),
        (" $0.024", PINK),
    ];
    let mut rows = if width >= 90 {
        vec![powerline(&segments)]
    } else {
        vec![powerline(&segments[..3]), powerline(&segments[3..])]
    };
    rows.push(Line::from(vec![
        piece("  ~", Color::Rgb(255, 154, 144)),
        piece(" / ", MUTED),
        piece("dev", Color::Rgb(255, 196, 102)),
        piece(" / ", MUTED),
        piece("agents", LIME),
        piece(" / ", MUTED),
        piece("coding", MINT),
        piece(" / ", MUTED),
        Span::styled(
            "plexmaton",
            Style::new().fg(TEAL).add_modifier(Modifier::BOLD),
        ),
    ]));
    let message = match mode {
        Mode::Quit => Some(piece("  press Ctrl-D again to quit", GOLD)),
        Mode::CommandHint => Some(piece("  press Ctrl-P for the command palette", SKY)),
        Mode::Ready | Mode::Approval => None,
    };
    if let (Some(message), Some(last_row)) = (message, rows.last_mut()) {
        *last_row = Line::from(message);
    }
    rows
}

fn plain(buffer: &Buffer) -> String {
    let mut result = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            result.push_str(buffer[(x, y)].symbol());
        }
        result.push('\n');
    }
    result
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn color(value: Color, fallback: &str) -> String {
    match value {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Black => "#000000".into(),
        Color::White => "#ffffff".into(),
        _ => fallback.into(),
    }
}
fn svg(buffer: &Buffer) -> String {
    let width = buffer.area.width * 9;
    let height = buffer.area.height * 19;
    let mut result = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%" viewBox="0 0 {width} {height}" preserveAspectRatio="xMinYMin meet"><rect width="100%" height="100%" fill="#0f1320"/><g font-family="Menlo,monospace" font-size="15">"##,
    );
    // Paint backgrounds first so a neighboring cell cannot erase a wide glyph.
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            let mut fg = color(cell.fg, "#eae4f2");
            let mut bg = color(cell.bg, "#0f1320");
            if cell.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let _ = write!(
                result,
                r#"<rect x="{}" y="{}" width="9" height="19" fill="{}"/>"#,
                x * 9,
                y * 19,
                bg
            );
        }
    }
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            let fg = if cell.modifier.contains(Modifier::REVERSED) {
                color(cell.bg, "#0f1320")
            } else {
                color(cell.fg, "#eae4f2")
            };
            if cell.symbol() != " " {
                // Powerline's private-use glyph needs a patched terminal font. Draw its exact
                // cell geometry in SVG so the review does not depend on Quick Look's fonts.
                if cell.symbol() == "\u{e0b0}" {
                    let _ = write!(
                        result,
                        r#"<polygon points="{},{} {},{} {},{}" fill="{}"/>"#,
                        x * 9,
                        y * 19,
                        x * 9 + 9,
                        y * 19 + 10,
                        x * 9,
                        y * 19 + 19,
                        fg
                    );
                    continue;
                }
                let weight = if cell.modifier.contains(Modifier::BOLD) {
                    "bold"
                } else {
                    "normal"
                };
                let opacity = if cell.modifier.contains(Modifier::DIM) {
                    "0.65"
                } else {
                    "1"
                };
                let _ = write!(
                    result,
                    r#"<text x="{}" y="{}" fill="{}" font-weight="{}" opacity="{}">{}</text>"#,
                    x * 9,
                    y * 19 + 15,
                    fg,
                    weight,
                    opacity,
                    escape(cell.symbol())
                );
            }
        }
    }
    result.push_str("</g></svg>");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_hints_replace_only_the_last_preview_row() -> Result<()> {
        // UI/UX §input: a global question stays on the terminal's last row.
        for width in [120, 95, 60] {
            let ready = preview(width, 26, Mode::Ready)?;
            for (mode, label) in [
                (Mode::Quit, "press Ctrl-D again to quit"),
                (Mode::CommandHint, "press Ctrl-P for the command palette"),
            ] {
                let hinted = preview(width, 26, mode)?;
                for y in 0..25 {
                    for x in 0..width {
                        assert_eq!(ready[(x, y)], hinted[(x, y)]);
                    }
                }
                let bottom: String = (0..width).map(|x| hinted[(x, 25)].symbol()).collect();
                assert_eq!(bottom.trim(), label);
            }
        }
        Ok(())
    }
}
