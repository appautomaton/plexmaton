//! One press slot for every surface with rows (INV-11): identity from press to release, and
//! nothing across surfaces or after a drag.
use super::*;
use crate::{
    Command, Page, Point, PointerIntent, SurfaceId, TuiIntent,
    intent::DrawerIntent,
    test_support::{canonical_runtime, region_text},
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use std::time::Instant;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn pointer(workspace: &mut Workspace, intent: PointerIntent) -> Outcome {
    workspace.apply(TuiIntent::Pointer(intent), Instant::now())
}

/// A drawn workspace with `/` listing the Commands above the composer.
fn with_commands() -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(95, 40)).expect("terminal");
    workspace.emit(canonical_runtime().ready(u64::MAX));
    workspace.settled_draw(&mut terminal).expect("draw");
    for _ in 0..=workspace.surfaces.len() {
        if workspace.state.focused(&workspace.surfaces) == Some(SurfaceId::Composer) {
            break;
        }
        workspace.handle(&key(KeyCode::Tab));
        workspace.settled_draw(&mut terminal).expect("draw");
    }
    workspace.handle(&key(KeyCode::Char('/')));
    workspace.settled_draw(&mut terminal).expect("menu");
    (workspace, terminal)
}

/// The cell on the row that shows `text`, inside `surface`.
fn row_at(
    workspace: &Workspace,
    terminal: &Terminal<TestBackend>,
    surface: SurfaceId,
    text: &str,
) -> Point {
    let bounds = workspace.surfaces.get(surface).expect("surface").bounds;
    let drawn = region_text(terminal.backend().buffer(), bounds);
    let (row, _) = drawn
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(text))
        .unwrap_or_else(|| panic!("{text:?} is a visible row:\n{drawn}"));
    Point {
        x: bounds.x + 4,
        y: bounds.y + u16::try_from(row).expect("row"),
    }
}

/// INV-11: a release on another surface, at the very cell the press landed on, activates nothing
/// and leaves no press behind; the next press and release on the row still act.
#[test]
fn a_press_on_one_surface_cannot_activate_a_release_on_another() {
    let (mut workspace, mut terminal) = with_commands();
    let at = row_at(&workspace, &terminal, SurfaceId::ComposerMenu, "/compact");
    let surface = SurfaceId::ComposerMenu;
    pointer(&mut workspace, PointerIntent::Press { surface, at });
    let elsewhere = pointer(
        &mut workspace,
        PointerIntent::Release {
            surface: SurfaceId::Transcript,
            at,
        },
    );
    assert!(
        elsewhere.command.is_none() && elsewhere.conversation.is_none(),
        "the row on the other surface did not activate"
    );
    assert!(
        workspace.pressed.is_none(),
        "the release consumed the press"
    );
    workspace.settled_draw(&mut terminal).expect("still listed");
    assert_eq!(workspace.state.composer().text(), "/");

    pointer(&mut workspace, PointerIntent::Press { surface, at });
    let run = pointer(&mut workspace, PointerIntent::Release { surface, at });
    assert_eq!(
        run.command.map(|run| run.command),
        Some(Command::Compact),
        "the same row, pressed and released on its own surface, runs"
    );
}

/// INV-11: a press, a drag that stays inside the row and a release on it activate nothing, on the
/// menu and on the Drawer alike; the row is still there to be pressed again.
#[test]
fn a_press_a_drag_inside_the_same_row_and_a_release_activate_nothing() {
    let (mut workspace, mut terminal) = with_commands();
    let surface = SurfaceId::ComposerMenu;
    let at = row_at(&workspace, &terminal, surface, "/new");
    let along = Point { x: at.x + 1, ..at };
    pointer(&mut workspace, PointerIntent::Press { surface, at });
    pointer(&mut workspace, PointerIntent::Drag { surface, at: along });
    let dragged = pointer(&mut workspace, PointerIntent::Release { surface, at });
    assert!(dragged.conversation.is_none() && dragged.command.is_none());
    assert!(workspace.pressed.is_none());
    workspace.settled_draw(&mut terminal).expect("menu stays");
    assert!(
        workspace.surfaces.get(surface).is_some(),
        "the menu is still open"
    );
    assert_eq!(workspace.state.composer().text(), "/");

    workspace.apply(TuiIntent::Drawer(DrawerIntent::Open), Instant::now());
    workspace.settled_draw(&mut terminal).expect("drawer");
    let surface = SurfaceId::Drawer;
    let at = row_at(&workspace, &terminal, surface, "Permissions");
    let along = Point { x: at.x + 1, ..at };
    pointer(&mut workspace, PointerIntent::Press { surface, at });
    pointer(&mut workspace, PointerIntent::Drag { surface, at: along });
    assert!(
        pointer(&mut workspace, PointerIntent::Release { surface, at })
            .page
            .is_none()
    );
    pointer(&mut workspace, PointerIntent::Press { surface, at });
    assert_eq!(
        pointer(&mut workspace, PointerIntent::Release { surface, at }).page,
        Some(Page::Permissions),
        "a clean press and release on the row still opens it"
    );
}
