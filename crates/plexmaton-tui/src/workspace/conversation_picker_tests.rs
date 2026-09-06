//! The Conversations page: New conversation first, saved history behind a query, one identity for
//! keyboard and pointer, and the Drawer's geometry and routing shared with every page.
use super::*;
use crate::{
    ConversationChoice, ConversationPickerStatus, ConversationRequest, Page, Point, SurfaceId,
};
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
    workspace.settled_draw(&mut terminal).expect("draw");
    (workspace, terminal)
}
fn panel(workspace: &Workspace, terminal: &Terminal<TestBackend>) -> String {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::Drawer)
        .expect("drawer")
        .bounds;
    crate::test_support::snapshot_text(terminal.backend().buffer(), bounds)
}

fn drawn_choice(workspace: &Workspace, terminal: &Terminal<TestBackend>, text: &str) -> Point {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::Drawer)
        .expect("drawer")
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

/// SPK-1: New conversation is the first row at every width, by keyboard and by pointer, and it
/// leaves as a request rather than a command. A listing that failed still offers it.
#[test]
fn new_conversation_is_the_first_row_by_keyboard_and_pointer() {
    for width in [120, 95, 60] {
        let (mut workspace, mut terminal) = setup(width);
        workspace.set_conversation_choices(choices(), false);
        for pointer in [false, true] {
            workspace.settled_draw(&mut terminal).expect("page");
            let drawn = panel(&workspace, &terminal);
            assert!(drawn.contains("Workspace · Conversations"), "{drawn}");
            assert!(drawn.contains("> New conversation"), "{drawn}");
            let outcome = if pointer {
                let point = drawn_choice(&workspace, &terminal, "> New conversation");
                workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
                workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), point))
            } else {
                workspace.handle(&key(KeyCode::Enter))
            };
            assert_eq!(outcome.conversation, Some(ConversationRequest::New));
            assert!(outcome.submitted.is_none() && outcome.page.is_none());
        }
        workspace.set_conversation_picker_status(ConversationPickerStatus::ListFailed);
        assert_eq!(
            workspace.handle(&key(KeyCode::Enter)).conversation,
            Some(ConversationRequest::New),
            "a history that cannot be read is no reason to refuse a fresh start"
        );
        workspace.set_conversation_picker_status(ConversationPickerStatus::Ready);
        workspace.handle(&Event::Paste("zzz".into()));
        assert!(
            workspace
                .handle(&key(KeyCode::Enter))
                .conversation
                .is_none()
        );
    }
}

/// SPK-1/INV-1: filtering, moving beyond the visible window and a click resolve the same stable
/// identity, a drag cancels, and `Escape` returns to the list rather than closing the Drawer.
#[test]
fn conversation_rows_keyboard_and_mouse_share_identity_and_cancel_drags() {
    for width in [120, 95, 60] {
        let (mut workspace, mut terminal) = setup(width);
        workspace.set_conversation_choices(choices(), false);
        for _ in 0..10 {
            workspace.handle(&key(KeyCode::Down));
        }
        workspace
            .settled_draw(&mut terminal)
            .expect("draw selected window");
        let chosen = workspace
            .handle(&key(KeyCode::Enter))
            .conversation
            .expect("keyboard selection");
        let id = ConversationId::new("conversation-09").expect("id");
        assert_eq!(chosen, ConversationRequest::Saved(id.clone()));
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
                .conversation
                .is_none()
        );
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
        assert_eq!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), point))
                .conversation,
            Some(ConversationRequest::Saved(id))
        );
        workspace.handle(&Event::Paste("conversation-03".into()));
        workspace.settled_draw(&mut terminal).expect("filtered");
        assert_eq!(
            workspace.handle(&key(KeyCode::Enter)).conversation,
            Some(ConversationRequest::Saved(
                ConversationId::new("conversation-03").expect("id")
            ))
        );
        workspace.set_conversation_picker_status(ConversationPickerStatus::Opening);
        let before = workspace.state().drawer().expect("page").clone();
        workspace.handle(&Event::Paste("ignored while opening".into()));
        workspace.handle(&key(KeyCode::Down));
        assert_eq!(workspace.state().drawer(), Some(&before));
        assert!(
            workspace
                .handle(&key(KeyCode::Enter))
                .conversation
                .is_none()
        );
        workspace.handle(&key(KeyCode::Esc));
        workspace.set_conversation_choices(choices(), false);
        assert!(
            !workspace.conversation_picker_open(),
            "late completion cannot reopen a dismissed page"
        );
        let drawer = workspace
            .state()
            .drawer()
            .expect("the list, one layer down");
        assert_eq!(drawer.page(), None);
    }
}

/// SPK-1: loading, empty history and a failed open, frozen as the page's region at each width.
#[test]
fn conversation_page_frames_cover_loading_empty_and_failure_states() {
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
            workspace.settled_draw(&mut terminal).expect("draw state");
            let drawn = panel(&workspace, &terminal);
            for signature in ["Workspace · Conversations", "New conversation", "Esc back"] {
                assert!(drawn.contains(signature), "{name} {state:?}: {drawn}");
            }
            frame.push_str(&drawn);
            frame.push('\n');
        }
        crate::test_support::assert_frame(&format!("drawer-conversations-{name}"), &frame);
    }
}

/// SPK-1/DRW-2: a short terminal still shows the keyboard-selected result and its controls.
#[test]
fn short_conversation_page_keeps_selected_result_and_footer_visible() {
    let (mut workspace, _) = setup(60);
    workspace.set_conversation_choices(choices(), false);
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("short terminal");
    for _ in 0..10 {
        workspace.handle(&key(KeyCode::Down));
    }
    workspace.settled_draw(&mut terminal).expect("short draw");
    let text = panel(&workspace, &terminal);
    assert!(text.contains("> Discuss project 09"), "{text}");
    assert!(
        text.contains("Enter open") && text.contains("Esc back"),
        "{text}"
    );
    workspace.set_conversation_picker_status(ConversationPickerStatus::OpenFailed);
    workspace
        .settled_draw(&mut terminal)
        .expect("failed short draw");
    assert!(panel(&workspace, &terminal).contains("Cannot open"));
    assert_eq!(
        workspace.handle(&key(KeyCode::Enter)).conversation,
        Some(ConversationRequest::Saved(
            ConversationId::new("conversation-09").expect("id")
        )),
        "a failed open is retried from the row that failed"
    );
}

/// DRW-2/DRW-3: a short Drawer keeps the marker and its keyboard affordances together, and the
/// wheel over it steps the choice.
#[test]
fn short_drawer_and_wheel_use_the_visible_choice_window() {
    let mut workspace = Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("terminal");
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )));
    workspace.settled_draw(&mut terminal).expect("draw");
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::Drawer)
        .expect("drawer")
        .bounds;
    workspace.handle(&mouse(
        MouseEventKind::ScrollDown,
        Point {
            x: bounds.x + 2,
            y: bounds.y + 2,
        },
    ));
    workspace
        .settled_draw(&mut terminal)
        .expect("draw selected");
    let drawn = panel(&workspace, &terminal);
    assert!(
        drawn.contains("> Conversations") && drawn.contains("Enter open"),
        "{drawn}"
    );
    assert_eq!(
        workspace.handle(&key(KeyCode::Enter)).page,
        Some(Page::Conversations)
    );
}
