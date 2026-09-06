//! The composer's height, and the window over a draft taller than it (ui-ux §input, COM-2).

use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    layout::Rect,
};

use super::*;
use crate::{
    SurfaceId,
    layout::composer_cap,
    test_support::{assert_frame, canonical_runtime, region_text},
};

fn drawn(width: u16, height: u16) -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(width, height))
        .unwrap_or_else(|error| panic!("test terminal: {error}"));
    workspace.emit(canonical_runtime().ready(u64::MAX));
    workspace
        .settled_draw(&mut terminal)
        .unwrap_or_else(|error| panic!("test render: {error}"));
    (workspace, terminal)
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn draw(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) -> bool {
    workspace
        .settled_draw(terminal)
        .unwrap_or_else(|error| panic!("test render: {error}"))
        .is_some()
}

fn focus_composer(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) {
    for _ in 0..=workspace.surfaces.len() {
        if workspace.state.focused(&workspace.surfaces) == Some(SurfaceId::Composer) {
            return;
        }
        workspace.handle(&key(KeyCode::Tab, KeyModifiers::NONE));
        draw(workspace, terminal);
    }
    panic!("the composer is a stop on the focus ring");
}

/// Types `text`, breaking lines without submitting.
fn typed(workspace: &mut Workspace, text: &str) {
    for character in text.chars() {
        if character == '\n' {
            workspace.handle(&key(KeyCode::Enter, KeyModifiers::SHIFT));
        } else {
            workspace.handle(&key(KeyCode::Char(character), KeyModifiers::NONE));
        }
    }
}

fn composer(workspace: &Workspace) -> Rect {
    workspace
        .surfaces
        .get(SurfaceId::Composer)
        .unwrap_or_else(|| panic!("the composer is registered"))
        .bounds
}

fn painted(terminal: &Terminal<TestBackend>, bounds: Rect) -> String {
    region_text(terminal.backend().buffer(), bounds)
}

fn caret_row(terminal: &Terminal<TestBackend>, composer: Rect) -> u16 {
    let backend = terminal.backend();
    assert!(backend.cursor_visible(), "the composer holds the cursor");
    backend
        .cursor_position()
        .y
        .checked_sub(composer.y + 1)
        .unwrap_or_else(|| panic!("the caret is inside the composer"))
}

fn lines(count: u16) -> String {
    (1..=count)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// COM-2 / ui-ux §input: the composer takes one row per wrapped line up to a third of the column,
/// then shows the window around the caret, which `↑` pulls up through the draft.
#[test]
fn a_draft_grows_to_a_third_of_the_column_then_its_window_follows_the_caret() {
    for (width, height) in [(120_u16, 40_u16), (95, 40), (60, 40)] {
        let cap = composer_cap(height);
        let (mut workspace, mut terminal) = drawn(width, height);
        focus_composer(&mut workspace, &mut terminal);
        for count in 1..=cap {
            typed(&mut workspace, &format!("line {count}"));
            draw(&mut workspace, &mut terminal);
            assert_eq!(
                composer(&workspace).height,
                count + 2,
                "{width}x{height}: {count} lines take {count} rows and two rules"
            );
            if count < cap {
                typed(&mut workspace, "\n");
            }
        }
        typed(
            &mut workspace,
            &format!("\n{}", lines(5).replace("line ", "more ")),
        );
        draw(&mut workspace, &mut terminal);
        let bounds = composer(&workspace);
        assert_eq!(bounds.height, cap + 2, "{width}x{height}: the cap holds");
        let shown = painted(&terminal, bounds);
        assert!(shown.contains("more 5") && !shown.contains("line 1\n"));
        assert_eq!(
            caret_row(&terminal, bounds),
            cap - 1,
            "the caret is on the newest row"
        );
        let conversation = workspace
            .surfaces
            .get(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("the conversation survives a tall draft"))
            .bounds;
        assert!(conversation.height >= 3);
        if width == 95 {
            assert_frame("composer-grown-medium", &shown);
        } else if width == 60 {
            assert_frame("composer-grown-narrow", &shown);
        }

        for _ in 0..cap {
            workspace.handle(&key(KeyCode::Up, KeyModifiers::NONE));
        }
        draw(&mut workspace, &mut terminal);
        let bounds = composer(&workspace);
        assert_eq!(bounds.height, cap + 2, "walking the draft costs no rows");
        let shown = painted(&terminal, bounds);
        assert_eq!(
            caret_row(&terminal, bounds),
            0,
            "the window pulled up to the caret"
        );
        assert!(
            shown.contains("line 5") && !shown.contains("more 5"),
            "{width}x{height}: the window moved with the caret:\n{shown}"
        );
        if width == 95 {
            assert_frame("composer-windowed-medium", &shown);
        } else if width == 60 {
            assert_frame("composer-windowed-narrow", &shown);
        }
    }
}

/// COM-2: the wheel over the composer walks the draft one row per notch while it holds the
/// caret, stops at the ends, and does nothing when the keyboard is elsewhere.
#[test]
fn the_wheel_over_the_composer_walks_the_draft_one_row_per_notch() {
    let (mut workspace, mut terminal) = drawn(95, 40);
    focus_composer(&mut workspace, &mut terminal);
    typed(&mut workspace, &lines(6));
    draw(&mut workspace, &mut terminal);
    let bounds = composer(&workspace);
    assert_eq!(caret_row(&terminal, bounds), 5);
    let wheel = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: bounds.x + 3,
            row: bounds.y + 2,
            modifiers: KeyModifiers::NONE,
        })
    };

    workspace.handle(&wheel(MouseEventKind::ScrollUp));
    assert!(draw(&mut workspace, &mut terminal));
    assert_eq!(caret_row(&terminal, bounds), 4);
    for _ in 0..10 {
        workspace.handle(&wheel(MouseEventKind::ScrollUp));
    }
    draw(&mut workspace, &mut terminal);
    assert_eq!(caret_row(&terminal, bounds), 0, "the first row is the end");
    workspace.handle(&wheel(MouseEventKind::ScrollUp));
    assert!(
        !draw(&mut workspace, &mut terminal),
        "a notch past the end changes nothing and costs no frame"
    );
    workspace.handle(&wheel(MouseEventKind::ScrollDown));
    assert!(draw(&mut workspace, &mut terminal));
    assert_eq!(caret_row(&terminal, bounds), 1);

    workspace.handle(&key(KeyCode::Tab, KeyModifiers::NONE));
    draw(&mut workspace, &mut terminal);
    assert_ne!(
        workspace.state.focused(&workspace.surfaces),
        Some(SurfaceId::Composer)
    );
    let before = workspace.state.clone();
    workspace.handle(&wheel(MouseEventKind::ScrollDown));
    assert_eq!(
        workspace.state, before,
        "without the caret the wheel over the composer moves nothing"
    );
}
