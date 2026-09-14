use super::*;
use plexmaton_core::{
    AgentId, AgentStatus, ConversationEvent, ConversationEventEnvelope, EventSequence,
};
use plexmaton_tui::SurfaceId;
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};

fn fixture() -> (Workspace, Terminal<TestBackend>, AgentId) {
    let mut workspace = Workspace::default();
    let agent = AgentId::new("preview").expect("fixture identity");
    workspace.emit(vec![ConversationEventEnvelope {
        sequence: EventSequence::new(1),
        event: ConversationEvent::AgentCreated {
            agent_id: agent.clone(),
            label: "Preview".into(),
            status: AgentStatus::Idle,
        },
    }]);
    let mut terminal = Terminal::new(TestBackend::new(88, 36)).expect("fixture terminal");
    for _ in 0..8 {
        workspace.draw(&mut terminal).expect("fixture draw");
        if workspace.state().focused(workspace.surfaces()) == Some(SurfaceId::Composer) {
            return (workspace, terminal, agent);
        }
        handle(&mut workspace, &key(KeyCode::Tab));
    }
    panic!("fixture must reach the primary composer");
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn screen(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) -> String {
    workspace.draw(terminal).expect("fixture draw");
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

/// COM-3: the absent preview runtime returns exact text instead of inventing delivery.
#[test]
fn unavailable_submission_preserves_the_draft() {
    let (mut workspace, mut terminal, agent) = fixture();
    let text = "Review e\u{301}vidence\nwithout a provider.";
    handle(&mut workspace, &Event::Paste(text.into()));
    workspace.draw(&mut terminal).expect("draft frame");
    handle(&mut workspace, &key(KeyCode::Enter));
    assert_eq!(workspace.state().draft(&agent).text(), text);
    assert!(screen(&mut workspace, &mut terminal).contains("Action unavailable"));
}

/// COM-3/SPK-2: rejected commands must retain their exact consumed draft.
#[test]
fn unavailable_commands_preserve_the_draft() {
    for command in ["/new", "/compact", "/tree"] {
        let (mut workspace, mut terminal, agent) = fixture();
        handle(&mut workspace, &Event::Paste(command.into()));
        workspace.draw(&mut terminal).expect("command frame");
        handle(&mut workspace, &key(KeyCode::Enter));
        assert_eq!(workspace.state().draft(&agent).text(), command, "{command}");
    }
}

/// SPK-3: an offline listing reaches failure instead of remaining in loading.
#[test]
fn unavailable_resume_listing_settles() {
    let (mut workspace, mut terminal, _) = fixture();
    handle(&mut workspace, &Event::Paste("/resume".into()));
    workspace.draw(&mut terminal).expect("command frame");
    handle(&mut workspace, &key(KeyCode::Enter));
    let frame = screen(&mut workspace, &mut terminal);
    assert!(frame.contains("Could not read saved conversations."));
    assert!(!frame.contains("Loading saved conversations"));
}

/// PER-7: the preview's absent permission owner returns an explicit unavailable view.
#[test]
fn unavailable_permissions_settle() {
    let (mut workspace, mut terminal, _) = fixture();
    handle(&mut workspace, &Event::Paste("/permissions".into()));
    workspace.draw(&mut terminal).expect("command frame");
    handle(&mut workspace, &key(KeyCode::Enter));
    let frame = screen(&mut workspace, &mut terminal);
    assert!(frame.contains("unavailable"), "{frame}");
    assert!(!frame.contains("Loading"));
}
