use super::*;
use crate::{ConversationChoice, ConversationPickerStatus, Point, SurfaceId};
use plexmaton_core::ConversationId;
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn mouse(kind: MouseEventKind, point: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: point.x,
        row: point.y,
        modifiers: KeyModifiers::NONE,
    })
}
fn choices() -> Vec<ConversationChoice> {
    (0..12)
        .map(|i| ConversationChoice {
            id: ConversationId::new(format!("conversation-{i:02}")).expect("id"),
            title: format!("Discuss project {i:02}"),
        })
        .collect()
}
fn setup(width: u16) -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::default();
    workspace.open_conversation_picker();
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
    workspace.draw(&mut terminal).expect("draw");
    (workspace, terminal)
}
fn panel(workspace: &Workspace, terminal: &Terminal<TestBackend>) -> String {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::CommandPalette)
        .expect("picker")
        .bounds;
    crate::test_support::snapshot_text(terminal.backend().buffer(), bounds)
}

fn drawn_choice(workspace: &Workspace, terminal: &Terminal<TestBackend>, text: &str) -> Point {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::CommandPalette)
        .expect("palette")
        .bounds;
    let drawn = panel(workspace, terminal);
    let (row, line) = drawn
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(text))
        .expect("visible choice");
    Point {
        x: bounds.x
            + u16::try_from(
                line.chars()
                    .position(|c| c == '>')
                    .expect("selected marker"),
            )
            .expect("column"),
        y: bounds.y + u16::try_from(row).expect("row"),
    }
}

/// INV-11: message actions cannot enter global discovery, while every resume alias selects one command.
#[test]
fn resume_aliases_share_one_command_and_retry_is_not_a_global_command() {
    for name in ["resume", "continue", "sessions", "session"] {
        assert_eq!(
            Command::from_slash(&format!("/{name}")),
            Some(Command::Resume)
        );
        let mut palette = crate::state::CommandPalette::opened_from(SurfaceId::Composer);
        for c in name.chars() {
            palette.filter_mut().insert(c);
        }
        assert_eq!(palette.matches(), vec![Command::Resume]);
    }
    for name in ["retry", "edit-retry"] {
        assert_eq!(Command::from_slash(&format!("/{name}")), None);
    }
    assert_eq!(
        Command::ALL,
        [
            Command::Config,
            Command::Resume,
            Command::New,
            Command::Permissions
        ]
    );
    assert_eq!(Command::from_slash("/new"), Some(Command::New));
}

/// SPK-1/INV-11: /new shares keyboard and pointer dispatch at every supported width.
#[test]
fn new_command_keyboard_and_pointer_emit_the_same_intent() {
    for width in [120, 95, 60] {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
        for pointer in [false, true] {
            workspace.handle(&Event::Key(KeyEvent::new(
                KeyCode::Char('p'),
                KeyModifiers::CONTROL,
            )));
            workspace.draw(&mut terminal).expect("opened palette");
            workspace.handle(&Event::Paste("/new".into()));
            workspace.draw(&mut terminal).expect("palette");
            assert!(panel(&workspace, &terminal).contains("/new  Start a new conversation"));
            let outcome = if pointer {
                let point = drawn_choice(&workspace, &terminal, "> /new");
                workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
                workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), point))
            } else {
                workspace.handle(&key(KeyCode::Enter))
            };
            assert_eq!(outcome.command, Some(Command::New));
            assert!(outcome.submitted.is_none());
            workspace.close_conversation_picker();
        }
    }
}

/// SPK-1/INV-1: filtering, moving beyond the visible window and a click resolve the same stable identity.
#[test]
fn session_picker_keyboard_and_mouse_share_identity_and_cancel_drags() {
    for width in [120, 95, 60] {
        let (mut workspace, mut terminal) = setup(width);
        workspace.set_conversation_choices(choices(), false);
        for _ in 0..9 {
            workspace.handle(&key(KeyCode::Down));
        }
        workspace.draw(&mut terminal).expect("draw selected window");
        let id = workspace
            .handle(&key(KeyCode::Enter))
            .resume
            .expect("keyboard selection");
        assert_eq!(id.as_str(), "conversation-09");
        let point = drawn_choice(&workspace, &terminal, "> Discuss project 09");
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
        workspace.handle(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            Point {
                x: point.x + 1,
                ..point
            },
        ));
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), point))
                .resume
                .is_none()
        );
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
        assert_eq!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), point))
                .resume,
            Some(id)
        );
        workspace.handle(&Event::Paste("conversation-03".into()));
        workspace.draw(&mut terminal).expect("filtered");
        assert_eq!(
            workspace
                .handle(&key(KeyCode::Enter))
                .resume
                .expect("filtered result")
                .as_str(),
            "conversation-03"
        );
        workspace.set_conversation_picker_status(ConversationPickerStatus::Opening);
        let before = workspace.state().command_palette().expect("picker").clone();
        workspace.handle(&Event::Paste("ignored while opening".into()));
        workspace.handle(&key(KeyCode::Down));
        assert_eq!(workspace.state().command_palette(), Some(&before));
        assert!(workspace.handle(&key(KeyCode::Enter)).resume.is_none());
        workspace.handle(&key(KeyCode::Esc));
        workspace.set_conversation_choices(choices(), false);
        assert!(
            !workspace.conversation_picker_open(),
            "late completion cannot reopen dismissed UI"
        );
    }
}

/// SPK-1: empty, populated and failure surfaces keep the search and controls visible at three widths.
#[test]
fn session_picker_frames_cover_empty_populated_and_failure_states() {
    let mut wide = None;
    for (width, name) in [(120, "wide"), (95, "medium"), (60, "narrow")] {
        let (mut workspace, mut terminal) = setup(width);
        let mut frame = String::new();
        for state in [
            ConversationPickerStatus::Loading,
            ConversationPickerStatus::Ready,
            ConversationPickerStatus::OpenFailed,
        ] {
            if state == ConversationPickerStatus::Ready {
                workspace.set_conversation_choices(Vec::new(), false);
            }
            if state == ConversationPickerStatus::OpenFailed {
                workspace.set_conversation_choices(choices(), false);
            }
            workspace.set_conversation_picker_status(state);
            workspace.draw(&mut terminal).expect("draw state");
            let drawn = panel(&workspace, &terminal);
            assert!(drawn.contains("Conversations") && drawn.contains("Esc close"));
            frame.push_str(&drawn);
            frame.push('\n');
        }
        if width == 95 {
            assert_eq!(
                wide.as_deref(),
                Some(frame.as_str()),
                "the capped panel is identical at medium width"
            );
        } else {
            crate::test_support::assert_frame(&format!("session-picker-{name}"), &frame);
            if width == 120 {
                wide = Some(frame);
            }
        }
    }
}

/// SPK-1/INV-13: a short terminal still shows the keyboard-selected result and its controls.
#[test]
fn short_session_picker_keeps_selected_result_and_footer_visible() {
    let (mut workspace, _) = setup(60);
    workspace.set_conversation_choices(choices(), false);
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("short terminal");
    for _ in 0..9 {
        workspace.handle(&key(KeyCode::Down));
    }
    workspace.draw(&mut terminal).expect("short draw");
    let text = panel(&workspace, &terminal);
    assert!(text.contains("> Discuss project 09"), "{text}");
    assert!(
        text.contains("Enter resume") && text.contains("Esc close"),
        "{text}"
    );
    workspace.set_conversation_picker_status(ConversationPickerStatus::OpenFailed);
    workspace.draw(&mut terminal).expect("failed short draw");
    assert!(panel(&workspace, &terminal).contains("Cannot open"));
    assert!(
        workspace.handle(&key(KeyCode::Enter)).resume.is_none(),
        "cannot activate an invisible choice"
    );
}

/// INV-11/INV-13: small command palettes keep the selected command and its keyboard affordances together.
#[test]
fn short_command_palette_and_wheel_use_the_visible_choice_window() {
    let mut workspace = Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("terminal");
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )));
    workspace.draw(&mut terminal).expect("draw");
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::CommandPalette)
        .expect("palette")
        .bounds;
    workspace.handle(&mouse(
        MouseEventKind::ScrollDown,
        Point {
            x: bounds.x + 2,
            y: bounds.y + 2,
        },
    ));
    workspace.draw(&mut terminal).expect("draw selected");
    let drawn = panel(&workspace, &terminal);
    assert!(
        drawn.contains("> /resume") && drawn.contains("Enter run"),
        "{drawn}"
    );
    assert_eq!(
        workspace.handle(&key(KeyCode::Enter)).command,
        Some(Command::Resume)
    );
}
