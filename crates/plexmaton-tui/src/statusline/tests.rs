use super::*;
use proptest::prelude::*;

fn plain(value: &StatusLineText) -> Vec<String> {
    value
        .lines()
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}

#[test]
fn statusline_styles_and_resets_preserve_text() {
    // STL-1: actual script styles survive, but their escape bytes do not.
    let text = StatusLineText::parse(
        b"\x1b[1;38;2;179;148;255;48;5;17mLuna\x1b[22;39;49m plain\n\x1b[3;4;7;9;96;101mstyled\x1b[23;24;27;29m reset\x1b[0m base",
    ).expect("supported SGR");
    assert_eq!(plain(&text), ["Luna plain", "styled reset base"]);
    let first = text.lines()[0].spans[0].style;
    assert_eq!(first.fg, Some(Color::Rgb(179, 148, 255)));
    assert_eq!(first.bg, Some(Color::Indexed(17)));
    assert!(first.add_modifier.contains(Modifier::BOLD));
    let reset = text.lines()[0].spans[1].style;
    assert_eq!(reset.fg, None);
    assert_eq!(reset.bg, None);
    assert!(!reset.add_modifier.contains(Modifier::BOLD));
    let second = text.lines()[1].spans[0].style;
    assert_eq!(second.fg, Some(Color::LightCyan));
    assert_eq!(second.bg, Some(Color::LightRed));
    assert!(second.add_modifier.contains(
        Modifier::ITALIC | Modifier::UNDERLINED | Modifier::REVERSED | Modifier::CROSSED_OUT
    ));
    assert!(text.lines()[1].spans[1].style.add_modifier.is_empty());
    assert_eq!(text.lines()[1].spans[2].style, Style::default());
    assert_eq!(
        StatusLineText::parse(b"plain").expect("text").lines()[0].spans[0].style,
        Style::default()
    );

    for (parameters, color) in [
        ("31", Color::Red),
        ("97", Color::White),
        ("38;5;255", Color::Indexed(255)),
        ("38;2;0;255;42", Color::Rgb(0, 255, 42)),
    ] {
        let parsed = StatusLineText::parse(format!("\x1b[{parameters}mx\nx").as_bytes())
            .expect("color grammar");
        assert!(
            parsed
                .lines()
                .iter()
                .all(|line| line.spans[0].style.fg == Some(color))
        );
    }
}

#[test]
fn statusline_rejects_terminal_effects_and_malformed_styles() {
    // STL-1: no partial acceptance, and errors cannot echo payload into diagnostic output.
    for source in [
        "\x1b]52;c;c2VjcmV0\x07",
        "\x1bPpayload\x1b\\",
        "\x1b[2J",
        "\x1b[H",
        "\x1b[?25l",
        "\x1b[5m",
        "\x1b[8m",
        "\x1b[38;2;1;2m",
        "\x1b[48;5;256m",
        "\x1b[38;2;256;0;0m",
        "\x1b[38:2:0:1:2m",
        "\x1b[+1m",
        "\x1b[65536m",
        "\x1b[",
        "\x1b",
        "\t",
        "\r",
        "\0",
        "\x7f",
        "\u{009b}",
        "\u{0085}",
    ] {
        let source = format!("accepted-looking prefix{source}secret-marker");
        let error = StatusLineText::parse(source.as_bytes()).expect_err("unsupported output");
        assert!(!format!("{error:?}: {error}").contains("secret-marker"));
        assert!(!error.to_string().contains('\x1b'));
    }
    assert_eq!(
        StatusLineText::parse(&[0xff]),
        Err(StatusLineTextError::InvalidUtf8)
    );
}

#[test]
fn statusline_bounds_bytes_rows_and_parameters() {
    // STL-1: bounded before allocation/parsing; all limits have a passing boundary too.
    assert!(StatusLineText::parse(&vec![b'x'; MAX_BYTES]).is_ok());
    assert_eq!(
        StatusLineText::parse(&vec![b'x'; MAX_BYTES + 1]),
        Err(StatusLineTextError::TooManyBytes)
    );
    assert!(StatusLineText::parse("x\n".repeat(MAX_ROWS).as_bytes()).is_ok());
    assert_eq!(
        StatusLineText::parse("x\n".repeat(MAX_ROWS + 1).as_bytes()),
        Err(StatusLineTextError::TooManyRows)
    );
    assert!(
        StatusLineText::parse(format!("\x1b[{}0mx", "0;".repeat(MAX_PARAMETERS - 1)).as_bytes())
            .is_ok()
    );
    assert_eq!(
        StatusLineText::parse(format!("\x1b[{}0mx", "0;".repeat(MAX_PARAMETERS)).as_bytes()),
        Err(StatusLineTextError::InvalidStyle)
    );
    assert!(
        StatusLineText::parse(format!("\x1b[{}mx", "0".repeat(MAX_SGR_BYTES - 1)).as_bytes())
            .is_ok()
    );
    assert_eq!(
        StatusLineText::parse(format!("\x1b[{}mx", "0".repeat(MAX_SGR_BYTES)).as_bytes()),
        Err(StatusLineTextError::InvalidStyle)
    );
}

#[test]
fn statusline_preserves_explicit_rows_and_spaces() {
    // STL-1: shell printf/echo endings, Powerline padding, Unicode and deliberate blank rows.
    for (source, expected) in [
        ("", vec![]),
        ("\n", vec![""]),
        ("one\n", vec!["one"]),
        ("one\r\n\r\n", vec!["one", ""]),
        (
            "  模型 🌸\n\n e\u{0301}  ",
            vec!["  模型 🌸", "", " e\u{0301}  "],
        ),
    ] {
        assert_eq!(
            plain(&StatusLineText::parse(source.as_bytes()).expect("plain text")),
            expected
        );
    }
    let reset = StatusLineText::parse(b"\x1b[31mred\x1b[mplain\x1b[;mbase").expect("empty reset");
    assert_eq!(reset.lines()[0].spans[1].style, Style::default());
    assert_eq!(reset.lines()[0].spans[2].style, Style::default());
}

#[test]
fn statusline_resets_restore_the_renderers_base_style() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget},
    };

    // STL-1: reset returns to semantic theme colors, without leaking the previous span's style.
    let output = StatusLineText::parse(b"\x1b[1;31;44mA\x1b[39mB\x1b[49mC\x1b[22mD\x1b[0mE")
        .expect("supported resets");
    let base = Style::default()
        .fg(Color::Rgb(136, 180, 255))
        .bg(Color::Rgb(15, 19, 32))
        .add_modifier(Modifier::ITALIC);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 5, 1));
    Paragraph::new(output.lines().to_vec())
        .style(base)
        .render(buffer.area, &mut buffer);
    assert_eq!(buffer[(0, 0)].fg, Color::Red);
    assert_eq!(buffer[(0, 0)].bg, Color::Blue);
    for x in 1..5 {
        assert_eq!(buffer[(x, 0)].fg, Color::Rgb(136, 180, 255));
        assert!(buffer[(x, 0)].modifier.contains(Modifier::ITALIC));
    }
    assert_eq!(buffer[(1, 0)].bg, Color::Blue);
    for x in 2..5 {
        assert_eq!(buffer[(x, 0)].bg, Color::Rgb(15, 19, 32));
    }
    for x in 0..3 {
        assert!(buffer[(x, 0)].modifier.contains(Modifier::BOLD));
    }
    for x in 3..5 {
        assert!(!buffer[(x, 0)].modifier.contains(Modifier::BOLD));
    }
}

proptest! {
    #[test]
    fn statusline_arbitrary_bytes_never_escape_as_controls(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        // STL-1: every accepted span is inert text, even for malformed external bytes.
        if let Ok(value) = StatusLineText::parse(&bytes) {
            prop_assert!(value.lines().len() <= MAX_ROWS);
            for row in value.lines() {
                for span in &row.spans {
                    prop_assert!(!span.content.chars().any(char::is_control));
                }
            }
        }
    }
}

#[test]
fn status_footer_preserves_focus_and_uses_the_last_row_for_hints() {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
    };
    // STL-4, FR-1: equal data costs no frame; hint placement cannot move input or script rows.
    for width in [120, 95, 60] {
        let mut workspace = crate::Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 26)).expect("terminal");
        workspace.draw(&mut terminal).expect("draw");
        let focus = workspace.state().focused(workspace.surfaces());
        let text = StatusLineText::parse(b"tokens\nrainbow path").expect("text");
        workspace.set_status_line(text.clone(), 6);
        assert!(workspace.draw(&mut terminal).expect("draw").is_some());
        assert_eq!(workspace.state().focused(workspace.surfaces()), focus);
        workspace.set_status_line(text, 6);
        assert!(workspace.draw(&mut terminal).expect("draw").is_none());
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
        )));
        workspace.draw(&mut terminal).expect("quit frame");
        let row = |y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        };
        assert!(row(24).starts_with("tokens"));
        assert_eq!(row(25).trim(), "press Ctrl-D again to quit");
    }
}

#[test]
fn status_footer_clipping_reserves_a_cell_before_a_wide_grapheme() {
    use ratatui::{Terminal, backend::TestBackend};
    // STL-4: TestBackend's terminal diff skips wide continuation cells, just like a real draw.
    let mut workspace = crate::Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("terminal");
    let input = format!("{}中\nhidden", "x".repeat(46));
    workspace.set_status_line(StatusLineText::parse(input.as_bytes()).expect("text"), 1);
    workspace.draw(&mut terminal).expect("draw");
    assert_eq!(terminal.backend().buffer()[(47, 11)].symbol(), "…");
    assert_ne!(terminal.backend().buffer()[(46, 11)].symbol(), "中");
    assert_eq!(
        workspace
            .surfaces()
            .get(crate::SurfaceId::Status)
            .expect("footer")
            .bounds
            .height,
        1
    );
}
