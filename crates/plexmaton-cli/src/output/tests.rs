use super::*;
use crate::clipboard::ClipboardRoute;
use plexmaton_tui::math::{
    FontStyle, GlyphRun, MathPaint, NativeStage, NativeText, TextScale, VerticalAlign,
};
use ratatui::{
    backend::Backend as _,
    buffer::Cell,
    style::{Color, Modifier, Style},
};

fn script() -> NativeText {
    NativeText {
        glyph: GlyphRun {
            x: 4,
            y: 2,
            text: "ij".into(),
            columns: 2,
            rows: 1,
            scale: TextScale::Script,
            align: VerticalAlign::Bottom,
            style: FontStyle::Italic,
            paint: MathPaint::Rgb {
                red: 1,
                green: 2,
                blue: 3,
            },
        },
        style: Style::new()
            .bg(Color::Black)
            .add_modifier(Modifier::REVERSED),
    }
}

/// MTH-5/PRE-1/SEL-8: all three production encoders write in order to one physical byte sink.
#[test]
fn cells_native_math_and_clipboard_share_one_ordered_output_owner() {
    let writer = Writer(Rc::new(RefCell::new(Vec::new())));
    let mut backend = CrosstermBackend::new(writer.clone());
    let mut clipboard = TerminalClipboard::new(writer.clone(), ClipboardRoute::Direct);
    write_native(&mut backend, NativeStage::Begin).expect("begin");
    let cell = Cell::new("body");
    backend
        .draw([(0, 0, &cell)].into_iter())
        .expect("actual cell encoder");
    write_native(
        &mut backend,
        NativeStage::End {
            changed: &[&script()],
            current: &[],
        },
    )
    .expect("native encoder");
    clipboard
        .submit(r"\(x_{ij}\)".into())
        .expect("actual clipboard encoder");
    let bytes = writer.0.borrow();
    let output = std::str::from_utf8(&bytes).expect("UTF-8 control stream");
    assert!(output.starts_with("\x1b[?2026h"));
    let body = output.find("body").expect("cells");
    let native = output
        .find("\x1b]66;s=1:n=7:d=10:v=1:w=2;ij\x07")
        .expect("sized text");
    let copy = output.find("\x1b]52;").expect("clipboard");
    assert!(body < native && native < copy);
    assert!(
        output[native..copy].contains("\x1b[?2026l"),
        "clipboard cannot interleave a frame"
    );
    assert!(
        output[..native].contains("\x1b[38;2;1;2;3m"),
        "explicit mathematical paint: {output:?}"
    );
}

/// MTH-2/MTH-4: no malformed native frame can write a valid-looking prefix before rejection.
#[test]
fn native_encoder_rejects_invalid_scale_controls_and_capacity_before_output() {
    let original = script();
    let mut bad = original.clone();
    bad.glyph.text = "\x1b]52;c;bad\x07".into();
    let mut scale = original.clone();
    scale.glyph.rows = 2;
    let mut width = original.clone();
    width.glyph.columns = 8;
    let mut text = original.clone();
    text.glyph.text = "a".repeat(4097);
    for bad in [bad, scale, width, text] {
        let mut bytes = Vec::new();
        assert!(
            write_native(
                &mut bytes,
                NativeStage::End {
                    changed: &[&original, &bad],
                    current: &[]
                }
            )
            .is_err()
        );
        assert!(bytes.is_empty());
    }
    let mut bytes = Vec::new();
    assert!(
        write_native(
            &mut bytes,
            NativeStage::End {
                changed: &vec![&original; 513],
                current: &[]
            }
        )
        .is_err()
    );
    assert!(bytes.is_empty());
}

/// MTH-5: neither an environment name, a width-only result nor a late unrelated CPR proves sizing.
#[test]
fn native_capability_requires_the_complete_measured_cursor_sequence() {
    assert_eq!(classify([(2, 2), (4, 2), (6, 2)]), MathPresentation::Native);
    for positions in [[(2, 2), (4, 2), (5, 2)], [(2, 2), (2, 2), (2, 2)]] {
        assert_eq!(
            classify(positions),
            MathPresentation::Source(MathUnavailable::Unsupported)
        );
    }
    for positions in [
        [(2, 2), (4, 3), (6, 3)],
        [(0, 0), (2, 0), (4, 0)],
        [(2, 2), (6, 2), (4, 2)],
    ] {
        assert_eq!(classify(positions), MathPresentation::default());
    }
}
