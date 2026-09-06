//! Tool disclosure/selection review through the real input router and renderer; no tool executes.
//! cargo run -p plexmaton-tui --example tool_disclosure_preview -- <output-directory>

use std::path::Path;

use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence, ToolCallId,
    ToolCallStatus, ToolDetail, ToolPresentation, TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{MarkdownTheme, Palette, Workspace};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
};

#[path = "support/frame_svg.rs"]
mod frame_svg;
#[path = "support/prepared_frame.rs"]
mod prepared_frame;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const PATCH: &str = "*** Begin Patch\n*** Update File: config.toml\n@@ bytes 0..10; old_bytes=10; new_bytes=10 @@\n-old = true\n+new = true\n*** End Patch";

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for (width, height) in [(120, 40), (88, 42), (60, 46)] {
        let mut workspace = fixture(false)?;
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        prepared_frame::draw(&mut workspace, &mut terminal)?;
        save(directory, "collapsed", &terminal)?;
        let at = point(&terminal, "exec_command")?;
        gesture(&mut workspace, &mut terminal, MouseEventKind::Moved, at)?;
        save(directory, "hover", &terminal)?;
        gesture(
            &mut workspace,
            &mut terminal,
            MouseEventKind::Down(MouseButton::Left),
            at,
        )?;
        gesture(
            &mut workspace,
            &mut terminal,
            MouseEventKind::Up(MouseButton::Left),
            at,
        )?;
        gesture(
            &mut workspace,
            &mut terminal,
            MouseEventKind::Moved,
            (width - 1, 0),
        )?;
        save(directory, "open", &terminal)?;
        let at = point(&terminal, "Cargo.toml")?;
        gesture(
            &mut workspace,
            &mut terminal,
            MouseEventKind::Down(MouseButton::Left),
            at,
        )?;
        let end = (at.0 + 10, at.1);
        gesture(
            &mut workspace,
            &mut terminal,
            MouseEventKind::Drag(MouseButton::Left),
            end,
        )?;
        let outcome = workspace.handle(&pointer(MouseEventKind::Up(MouseButton::Left), end));
        if outcome.copied.as_ref().map(|copy| copy.text.as_str()) != Some("Cargo.toml") {
            return Err("the actual pointer range did not copy the exact fixture source".into());
        }
        prepared_frame::draw(&mut workspace, &mut terminal)?;
        save(directory, "text-selected", &terminal)?;
        let mut workspace = fixture(true)?;
        prepared_frame::draw(&mut workspace, &mut terminal)?;
        for (key, modifiers) in [
            (KeyCode::Up, KeyModifiers::SHIFT),
            (KeyCode::Char('o'), KeyModifiers::CONTROL),
        ] {
            workspace.handle(&Event::Key(KeyEvent::new(key, modifiers)));
            prepared_frame::draw(&mut workspace, &mut terminal)?;
        }
        let copied = workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('y'),
            KeyModifiers::CONTROL,
        )));
        if copied.copied.as_ref().map(|copy| copy.text.as_str())
            != Some(&format!("path: config.toml\n{PATCH}"))
        {
            return Err("whole diff selection did not preserve its exact source".into());
        }
        prepared_frame::draw(&mut workspace, &mut terminal)?;
        save(directory, "diff-selected", &terminal)?;
    }
    Ok(())
}

fn fixture(diff: bool) -> Result<Workspace> {
    let agent = AgentId::new("primary")?;
    let question = TranscriptItemId::new("question")?;
    let mut events = vec![
        ConversationEvent::AgentCreated {
            agent_id: agent.clone(),
            label: "Plexmaton".into(),
            status: AgentStatus::Idle,
        },
        ConversationEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: question.clone(),
            role: TranscriptRole::User,
        },
        ConversationEvent::TranscriptDelta {
            agent_id: agent.clone(),
            item_id: question,
            item_revision: 1,
            text: "What do we have for this project?".into(),
        },
    ];
    for (i, label) in ["exec_command", "read_file", "read_file", "read_file"]
        .into_iter()
        .enumerate()
    {
        let id = format!("tool-{i}");
        let item = TranscriptItemId::new(&id)?;
        let call = ToolCallId::new(&id)?;
        for (revision, status) in [
            (0, ToolCallStatus::Queued),
            (1, ToolCallStatus::Running),
            (2, ToolCallStatus::Succeeded),
        ] {
            let presentation = if revision == 0 {
                ToolPresentation::default()
            } else {
                ToolPresentation {
                    invocation: Some(ToolDetail::Text {
                        source: if i == 0 { "command: rg --files\ncwd: /workspace/plexmaton\ntimeout_ms: 120000".into() } else { "path: .agents/roadmap.md".into() },
                        omitted_bytes: 0,
                    }),
                    outcome: (revision == 2).then(|| ToolDetail::Text {
                        source: if i == 0 {
                            "status: exited\nexit_code: 0\nstdout_complete: true\nstdout:\nCargo.toml\nREADME.md\n.agents/roadmap.md\n.agents/ui-ux.md\ncrates/plexmaton-tui/src/workspace.rs\nscripts/smoke-tui.py".into()
                        } else { "Plexmaton roadmap: responsive, durable sessions.".into() },
                        omitted_bytes: 0,
                    }),
                }
            };
            events.push(ConversationEvent::ToolCallChanged {
                agent_id: agent.clone(),
                item_id: item.clone(),
                item_revision: revision,
                call_id: call.clone(),
                label: label.into(),
                status,
                presentation,
            });
        }
    }
    if diff {
        for (revision, status) in [
            (0, ToolCallStatus::Queued),
            (1, ToolCallStatus::Running),
            (2, ToolCallStatus::Succeeded),
        ] {
            events.push(ConversationEvent::ToolCallChanged {
                agent_id: agent.clone(),
                item_id: TranscriptItemId::new("diff")?,
                item_revision: revision,
                call_id: ToolCallId::new("diff")?,
                label: "apply_patch".into(),
                status,
                presentation: ToolPresentation {
                    invocation: Some(ToolDetail::Text {
                        source: "path: config.toml".into(),
                        omitted_bytes: 0,
                    }),
                    outcome: (revision == 2).then(|| ToolDetail::Diff {
                        patch: PATCH.into(),
                    }),
                },
            });
        }
    }
    let mut workspace =
        Workspace::with_palette(Palette::ansi().with_markdown_theme(MarkdownTheme::Pastel));
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

fn pointer(kind: MouseEventKind, (column, row): (u16, u16)) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn gesture(
    workspace: &mut Workspace,
    terminal: &mut Terminal<TestBackend>,
    kind: MouseEventKind,
    at: (u16, u16),
) -> Result<()> {
    workspace.handle(&pointer(kind, at));
    prepared_frame::draw(workspace, terminal)?;
    Ok(())
}

fn point(terminal: &Terminal<TestBackend>, text: &str) -> Result<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width.saturating_sub(text.len() as u16) {
            if text
                .chars()
                .enumerate()
                .all(|(i, c)| buffer[(x + i as u16, y)].symbol() == c.to_string())
            {
                return Ok((x, y));
            }
        }
    }
    Err(format!("fixture text {text:?} was not visible").into())
}

fn save(directory: &Path, name: &str, terminal: &Terminal<TestBackend>) -> Result<()> {
    let buffer = terminal.backend().buffer();
    std::fs::write(
        directory.join(format!("{name}-{}.svg", buffer.area.width)),
        frame_svg::svg(buffer),
    )?;
    Ok(())
}
