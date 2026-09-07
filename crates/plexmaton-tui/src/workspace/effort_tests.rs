use super::composer_menu_tests::{key, mouse, setup, typed};
use super::*;
use crate::{Point, SurfaceId};
use plexmaton_core::ReasoningEffort as Effort;
use ratatui::crossterm::event::{KeyCode, MouseButton, MouseEventKind};

fn open(
    width: u16,
    allowed: &[Effort],
    current: Effort,
) -> (Workspace, Terminal<ratatui::backend::TestBackend>) {
    let (mut workspace, mut terminal) = setup(width);
    let mut model = crate::test_support::configuration_summary();
    model.reasoning_effort = current;
    workspace.set_model(model);
    workspace.set_effort_choices(Some(allowed.to_vec()));
    typed(&mut workspace, "/effort ");
    workspace
        .settled_draw(&mut terminal)
        .expect("selector frame");
    (workspace, terminal)
}

/// EFF-2: keyboard selection skips disabled stops, and only runtime acceptance changes effort.
#[test]
fn effort_selection_confirms_only_after_runtime_acceptance_and_escape_cancels() {
    for width in [60, 88, 120] {
        let (mut workspace, mut terminal) = open(width, &[Effort::Low, Effort::High], Effort::High);
        assert_eq!(workspace.state.selected_effort(), Some(Effort::High));
        workspace.handle(&key(KeyCode::Left));
        assert_eq!(workspace.state.selected_effort(), Some(Effort::Low));
        assert_eq!(workspace.state.reasoning_effort(), Some(Effort::High));
        let change = workspace
            .handle(&key(KeyCode::Enter))
            .effort
            .expect("addressed change");
        assert_eq!(change.effort, Effort::Low);
        workspace.report_effort(Err("Wait for current work.".to_owned()));
        assert_eq!(workspace.state.reasoning_effort(), Some(Effort::High));
        workspace.handle(&key(KeyCode::Esc));
        assert!(!workspace.state.effort_visible());
        assert_eq!(workspace.state.reasoning_effort(), Some(Effort::High));
        workspace.handle(&key(KeyCode::Backspace));
        workspace.handle(&key(KeyCode::Backspace));
        typed(&mut workspace, "t ");
        workspace.settled_draw(&mut terminal).expect("reopen");
        let mut model = crate::test_support::configuration_summary();
        model.reasoning_effort = Effort::Low;
        workspace.report_effort(Ok(model));
        assert_eq!(workspace.state.reasoning_effort(), Some(Effort::Low));
        assert!(workspace.state.composer().text().is_empty());
    }
}

/// EFF-2/INV-11: disabled stops cannot arm a press; an available click previews without applying.
#[test]
fn effort_pointer_ignores_disabled_stops_and_drag_disarms_selection() {
    let (mut workspace, mut terminal) = open(88, &[Effort::Low, Effort::High], Effort::High);
    let bounds = workspace
        .surfaces
        .get(SurfaceId::ComposerMenu)
        .expect("menu")
        .bounds;
    let row = bounds.y + 3;
    let low = (bounds.x..bounds.right())
        .map(|x| Point { x, y: row })
        .find(|point| {
            workspace.menu_hit(*point) == Some(crate::state::MenuRow::Effort(Effort::Low))
        })
        .expect("low target");
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), low));
    workspace.handle(&mouse(
        MouseEventKind::Drag(MouseButton::Left),
        Point {
            x: low.x + 1,
            y: low.y,
        },
    ));
    workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), low));
    assert_eq!(workspace.state.selected_effort(), Some(Effort::High));
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), low));
    workspace.handle(&key(KeyCode::Esc));
    workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), low));
    assert_eq!(workspace.state.selected_effort(), Some(Effort::High));
    assert!(
        workspace.state.effort_visible(),
        "Escape cancels the press before closing"
    );
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), low));
    let outcome = workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), low));
    assert!(outcome.effort.is_none());
    assert_eq!(workspace.state.selected_effort(), Some(Effort::Low));
    let disabled = Point {
        x: bounds.right() - 6,
        y: row,
    };
    assert!(workspace.menu_hit(disabled).is_none());
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), disabled));
    workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), disabled));
    assert_eq!(workspace.state.selected_effort(), Some(Effort::Low));
    workspace.settled_draw(&mut terminal).expect("selected");
}

/// EFF-2: provider default is an honest non-stop state until the user explicitly navigates.
#[test]
fn effort_provider_default_does_not_preselect_an_explicit_level() {
    let (mut workspace, _) = open(88, &[Effort::Low, Effort::High], Effort::Default);
    assert_eq!(workspace.state.selected_effort(), Some(Effort::Default));
    assert_eq!(
        workspace
            .handle(&key(KeyCode::Enter))
            .effort
            .expect("confirm current default")
            .effort,
        Effort::Default
    );
    workspace.handle(&key(KeyCode::Right));
    assert_eq!(workspace.state.selected_effort(), Some(Effort::Low));
    assert_eq!(workspace.state.reasoning_effort(), Some(Effort::Default));
}

/// EFF-3/EFF-4/FR-4: real frames keep transcript/rules/status static while visible max cells animate.
#[test]
fn effort_animation_changes_only_visible_max_cells_and_stops_when_hidden() {
    use unicode_width::UnicodeWidthStr;
    for shape in ["▲", "■", "⬢", "●"] {
        assert_eq!(shape.width(), 1);
    }
    for width in [60, 88, 120] {
        let (mut workspace, mut terminal) = open(width, &Effort::EXPLICIT, Effort::Max);
        let before = terminal.backend().buffer().clone();
        let revision = workspace.state.revision();
        let now = Instant::now();
        workspace
            .effort_animation_deadline(now)
            .expect("visible timer");
        workspace.advance_effort_animation(now + std::time::Duration::from_millis(1700));
        assert_eq!(workspace.state.revision(), revision);
        let work = workspace
            .draw(&mut terminal)
            .expect("animation frame")
            .expect("changed");
        assert_eq!(work.entries_wrapped, 0);
        let after = terminal.backend().buffer();
        let diff = before.diff(after);
        assert_eq!(
            diff.len(),
            7,
            "four selector cells and three composer letters at width {width}"
        );
        let menu = workspace
            .surfaces
            .get(SurfaceId::ComposerMenu)
            .expect("menu")
            .bounds;
        let composer = workspace
            .surfaces
            .get(SurfaceId::Composer)
            .expect("composer")
            .bounds;
        for (x, y, cell) in diff {
            assert!(y == menu.y + 3 || y == menu.y + 4 || y == composer.y);
            assert_eq!(before[(x, y)].bg, cell.bg);
        }
        workspace.handle(&key(KeyCode::Esc));
        workspace.draw(&mut terminal).expect("close");
        assert!(
            workspace.effort_animation_deadline(now).is_some(),
            "composer max remains visible"
        );
        let before = terminal.backend().buffer().clone();
        workspace.advance_effort_animation(now + std::time::Duration::from_millis(2000));
        workspace.draw(&mut terminal).expect("composer animation");
        let diff = before.diff(terminal.backend().buffer());
        assert_eq!(
            diff.len(),
            3,
            "only the composer max letters animate after closing"
        );
        for (x, y, cell) in diff {
            assert_eq!(y, composer.y);
            assert_eq!(before[(x, y)].symbol(), cell.symbol());
            assert_eq!(before[(x, y)].bg, cell.bg);
        }
        workspace.handle(&Event::Key(ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Char('p'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        )));
        workspace.draw(&mut terminal).expect("drawer");
        assert!(workspace.effort_animation_deadline(now).is_none());
        workspace.advance_effort_animation(now + std::time::Duration::from_secs(10));
        assert!(!workspace.needs_draw());
    }
}

/// EFF-3/EFF-4: static xhigh in both places owns no ambient wake.
#[test]
fn effort_xhigh_labels_remain_static_without_an_animation_deadline() {
    let (mut workspace, _) = open(88, &Effort::EXPLICIT, Effort::Xhigh);
    let now = Instant::now();
    assert!(workspace.effort_animation_deadline(now).is_none());
    workspace.advance_effort_animation(now + std::time::Duration::from_secs(10));
    assert!(!workspace.needs_draw());
}
