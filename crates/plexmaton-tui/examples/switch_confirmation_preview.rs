//! The switch a working child would lose, offered at every width it is read at (SPK-2).
//!
//! Two places the same question lands: `/resume`'s listing, where the row that asks it is also the
//! row that answers it, and `/new`, which has no row and is answered after the conversation's last
//! entry.
//!
//! Ultrawide is here because it is the width this question is most often read at — the user has a
//! child open beside the conversation, which is exactly when a switch has something to lose. It is
//! also the tightest: the menu belongs to the primary's column, so at 160 it has about 66 columns,
//! narrower than the 88-column single-column frame.
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
    ReasoningEffort,
};
use plexmaton_tui::{
    ChildControl, ChildControlSnapshot, ConfigurationSummary, ConversationChoice, Palette,
    SurfaceId, Workspace,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
};
use std::path::Path;

#[path = "support/frame_svg.rs"]
mod frame_svg;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).ok_or("provide output directory")?;
    let output = Path::new(&output);
    std::fs::create_dir_all(output)?;
    for width in [160, 120, 88, 60] {
        let mut workspace = Workspace::with_palette(Palette::pastel());
        workspace.emit(vec![
            ConversationEventEnvelope {
                sequence: EventSequence::new(1),
                event: ConversationEvent::AgentCreated {
                    agent_id: AgentId::new("primary")?,
                    label: "Plexmaton".into(),
                    status: AgentStatus::Idle,
                },
            },
            ConversationEventEnvelope {
                sequence: EventSequence::new(2),
                event: ConversationEvent::AgentCreated {
                    agent_id: AgentId::new("delegated-1")?,
                    label: "Delegated 1".into(),
                    status: AgentStatus::Running,
                },
            },
        ]);
        workspace.set_model(ConfigurationSummary {
            provider: "local".into(),
            configured_name: "luna".into(),
            model: "gpt-5.6-luna".into(),
            display_name: "gpt-5.6-luna".into(),
            reasoning_effort: ReasoningEffort::Max,
        });
        workspace.set_working_directory("~/dev/plexmaton · main".into());
        workspace.set_child_control(
            &AgentId::new("delegated-1")?,
            ChildControlSnapshot {
                revision: 1,
                control: ChildControl::Main,
            },
        )?;
        let mut terminal = Terminal::new(TestBackend::new(width, 30))?;
        workspace.draw(&mut terminal)?;
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Composer) {
                break;
            }
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
            workspace.draw(&mut terminal)?;
        }

        // At ultrawide the child earns a column of its own, so open it the way the user does:
        // focus the roster, move to the child, enter it, then come back to the composer.
        if width >= 132 {
            for _ in 0..workspace.surfaces().len() {
                if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Agents) {
                    break;
                }
                workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
                workspace.draw(&mut terminal)?;
            }
            workspace.handle(&Event::Key(KeyEvent::new(
                KeyCode::Down,
                KeyModifiers::NONE,
            )));
            workspace.handle(&Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE,
            )));
            workspace.draw(&mut terminal)?;
            for _ in 0..workspace.surfaces().len() {
                if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Composer) {
                    break;
                }
                workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
                workspace.draw(&mut terminal)?;
            }
        }

        // `/resume`: the rows stay, and the sentence sits under them.
        workspace.handle(&Event::Paste("/resume ".into()));
        workspace.open_conversation_picker();
        workspace.set_conversation_choices(
            [
                ("conversation-01a0aca5", "Explain the heat equation"),
                ("conversation-01a0a9f1", "Kirchhoff transform notes"),
                ("conversation-01a0a844", "Phase 03 control review"),
            ]
            .into_iter()
            .map(|(id, title)| ConversationChoice {
                id: plexmaton_core::ConversationId::new(id).expect("fixture id"),
                title: title.into(),
            })
            .collect(),
            false,
        );
        workspace.draw(&mut terminal)?;
        workspace.arm_switch("Delegated 1".into());
        workspace.draw(&mut terminal)?;
        std::fs::write(
            output.join(format!("switch-confirm-{width}.svg")),
            frame_svg::svg(terminal.backend().buffer()),
        )?;

        // `/new` has no row to choose again, so the same question lands as a note.
        let mut workspace = Workspace::with_palette(Palette::pastel());
        workspace.emit(vec![
            ConversationEventEnvelope {
                sequence: EventSequence::new(1),
                event: ConversationEvent::AgentCreated {
                    agent_id: AgentId::new("primary")?,
                    label: "Plexmaton".into(),
                    status: AgentStatus::Idle,
                },
            },
            ConversationEventEnvelope {
                sequence: EventSequence::new(2),
                event: ConversationEvent::AgentCreated {
                    agent_id: AgentId::new("delegated-1")?,
                    label: "Delegated 1".into(),
                    status: AgentStatus::Running,
                },
            },
        ]);
        workspace.set_working_directory("~/dev/plexmaton · main".into());
        workspace.draw(&mut terminal)?;
        for _ in 0..workspace.surfaces().len() {
            if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Composer) {
                break;
            }
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
            workspace.draw(&mut terminal)?;
        }
        workspace.handle(&Event::Paste("/new".into()));
        workspace.begin_conversation_switch();
        workspace.arm_switch("Delegated 1".into());
        workspace.draw(&mut terminal)?;
        std::fs::write(
            output.join(format!("switch-new-{width}.svg")),
            frame_svg::svg(terminal.backend().buffer()),
        )?;
    }
    Ok(())
}
