use super::composer_menu_tests::{key, mouse, setup, typed};
use super::*;
use crate::{Point, SurfaceId, state::MenuRow};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};

fn control(code: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL))
}

/// INV-3/DRW-3: one chosen page follows movement, arrows continue from it, Enter opens it.
#[test]
fn drawer_hover_and_arrows_share_one_choice_without_opening_pages() {
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal) = setup(width);
        workspace.handle(&control('p'));
        workspace.settled_draw(&mut terminal).expect("drawer");
        let bounds = workspace
            .surfaces
            .get(SurfaceId::Drawer)
            .expect("drawer")
            .bounds;
        let at = (bounds.y..bounds.bottom())
            .map(|y| Point { x: bounds.x + 5, y })
            .find(|at| {
                workspace.drawer_hit(*at) == Some(drawer::DrawerChoice::Page(Page::Permissions))
            })
            .expect("permission row");
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Moved, at))
                .page
                .is_none()
        );
        assert_eq!(workspace.state.chosen_page(), Some(Page::Permissions));
        workspace.handle(&key(KeyCode::Up));
        assert_eq!(workspace.state.chosen_page(), Some(Page::Configuration));
        workspace.handle(&mouse(MouseEventKind::Moved, at));
        assert_eq!(workspace.state.chosen_page(), Some(Page::Configuration));
        assert_eq!(
            workspace.handle(&key(KeyCode::Enter)).page,
            Some(Page::Configuration)
        );
    }
}

/// INV-3/CMC-2: composer menus share a chosen identity while leaving their query untouched.
#[test]
fn composer_menu_hover_preserves_draft_and_arrows_continue_from_the_hovered_row() {
    let (mut workspace, mut terminal) = setup(88);
    typed(&mut workspace, "/");
    workspace.settled_draw(&mut terminal).expect("menu");
    let bounds = workspace
        .surfaces
        .get(SurfaceId::ComposerMenu)
        .expect("menu")
        .bounds;
    let at = (bounds.y..bounds.bottom())
        .map(|y| Point { x: bounds.x + 4, y })
        .find(|at| workspace.menu_hit(*at) == Some(MenuRow::Command(crate::Command::Resume)))
        .expect("resume row");
    workspace.handle(&mouse(MouseEventKind::Moved, at));
    assert_eq!(
        workspace.state.menu_chosen(),
        Some(MenuRow::Command(crate::Command::Resume))
    );
    assert_eq!(workspace.state.composer().text(), "/");
    let rows = workspace.state.menu_rows();
    let index = rows
        .iter()
        .position(|row| *row == MenuRow::Command(crate::Command::Resume))
        .expect("row");
    workspace.handle(&key(KeyCode::Down));
    assert_eq!(workspace.state.menu_chosen(), rows.get(index + 1).cloned());
    workspace.handle(&mouse(MouseEventKind::Moved, at));
    assert_eq!(workspace.state.menu_chosen(), rows.get(index + 1).cloned());
}

/// DRW-3/INV-11: retract closes a page to origin; drag, keyboard and resize cancel held presses.
#[test]
fn drawer_retract_is_visible_on_pages_and_requires_an_unchanged_click() {
    for width in [120, 88, 60] {
        for cancel in 0..4 {
            let (mut workspace, mut terminal) = setup(width);
            workspace.handle(&control('p'));
            workspace.show_configuration(crate::test_support::configuration_summary());
            workspace.settled_draw(&mut terminal).expect("page");
            let bounds = workspace
                .surfaces
                .get(SurfaceId::Drawer)
                .expect("drawer")
                .bounds;
            let rect = crate::layout::drawer_retract_control(bounds);
            let at = Point {
                x: rect.x + 1,
                y: rect.y,
            };
            assert_eq!(terminal.backend().buffer()[(at.x, at.y)].symbol(), "⌃");
            workspace.handle(&mouse(MouseEventKind::Moved, at));
            assert!(workspace.state.drawer_retract_hovered());
            workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
            match cancel {
                1 => {
                    workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), at));
                }
                2 => {
                    workspace.handle(&key(KeyCode::Up));
                }
                3 => {
                    workspace.handle(&Event::Resize(width, 39));
                }
                _ => {}
            }
            workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), at));
            assert_eq!(workspace.state.drawer().is_none(), cancel == 0);
            if cancel == 0 {
                workspace.settled_draw(&mut terminal).expect("closed");
                assert_eq!(
                    workspace.state.focused(&workspace.surfaces),
                    Some(SurfaceId::Composer)
                );
            }
        }
    }
}
