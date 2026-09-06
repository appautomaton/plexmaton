use super::*;
use crate::test_support::Conversation;
use ratatui::{
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};

fn pointer(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// TR-1/MD-5/SEL-2: repainting colors keeps the drawn geometry, parked source and pointer selection.
#[test]
fn palette_changes_reuse_heights_and_preserve_pointer_copy_at_three_widths() {
    let base = Palette::ansi().with_markdown_theme(crate::MarkdownTheme::Pastel);
    for width in [120, 88, 60] {
        let mut conversation = Conversation::canonical();
        conversation.extend(500).append(
            "\n\n## Color keeps geometry\n\n**bold** and [link](https://example.com).\n\nEnd.",
        );
        let mut workspace = Workspace::with_palette(base);
        workspace.emit(conversation.drain());
        let mut terminal = Terminal::new(TestBackend::new(width, 44)).expect("terminal");
        workspace.settled_draw(&mut terminal).expect("warm");
        let bounds = workspace
            .surfaces
            .get(crate::SurfaceId::Transcript)
            .expect("transcript")
            .bounds;
        let (column, row) = (bounds.y + 1..bounds.bottom() - 1)
            .flat_map(|y| (bounds.x + 1..bounds.right().saturating_sub(4)).map(move |x| (x, y)))
            .find(|(x, y)| {
                (0..4)
                    .map(|offset| terminal.backend().buffer()[(*x + offset, *y)].symbol())
                    .collect::<String>()
                    == "bold"
            })
            .expect("visible bold word");
        workspace.handle(&pointer(
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
        ));
        workspace.handle(&pointer(
            MouseEventKind::Drag(MouseButton::Left),
            column + 4,
            row,
        ));
        let copied = workspace.handle(&pointer(
            MouseEventKind::Up(MouseButton::Left),
            column + 4,
            row,
        ));
        assert_eq!(copied.copied.expect("pointer copy").text, "bold");
        // Settle keyboard copy feedback/hover before comparing palette-only changes.
        assert_eq!(
            workspace
                .handle(&Event::Key(KeyEvent::new(
                    KeyCode::Char('y'),
                    KeyModifiers::CONTROL
                )))
                .copied
                .expect("initial keyboard copy")
                .text,
            "bold"
        );
        workspace
            .settled_draw(&mut terminal)
            .expect("selected frame");
        let before = terminal.backend().buffer().clone();
        let viewport = workspace
            .surfaces
            .viewport(crate::SurfaceId::Transcript)
            .expect("viewport");
        let retained = workspace.metrics.retained();
        let layouts = workspace.metrics.text_layouts();
        for palette in [Palette::monochrome(), Palette::pastel(), base] {
            let state = workspace.state.clone();
            workspace.set_palette(palette);
            let work = workspace
                .settled_draw(&mut terminal)
                .expect("paint")
                .expect("changed palette");
            assert_eq!(work.entries_wrapped, 0, "color invalidated height geometry");
            assert_eq!(
                workspace.metrics.text_layouts(),
                layouts,
                "color rebuilt a prepared text map"
            );
            assert_eq!(workspace.metrics.retained(), retained);
            assert_eq!(workspace.state, state);
            assert_eq!(
                workspace.surfaces.viewport(crate::SurfaceId::Transcript),
                Some(viewport)
            );
            for (old, new) in before
                .content
                .iter()
                .zip(&terminal.backend().buffer().content)
            {
                assert_eq!(old.symbol(), new.symbol(), "palette moved visible content");
            }
            workspace.set_palette(palette);
            assert!(
                workspace
                    .settled_draw(&mut terminal)
                    .expect("no change")
                    .is_none()
            );
            assert_eq!(
                workspace
                    .handle(&Event::Key(KeyEvent::new(
                        KeyCode::Char('y'),
                        KeyModifiers::CONTROL
                    )))
                    .copied
                    .expect("copy remains")
                    .text,
                "bold"
            );
            workspace
                .settled_draw(&mut terminal)
                .expect("copy feedback");
        }
        assert_eq!(
            terminal.backend().buffer(),
            &before,
            "palette round trip changed the frame"
        );
    }
}
