use super::*;
use crate::markdown;

/// MD-1/MD-2/SEL-2: visible text is parser output, independent of wrapping and UI adornments.
#[test]
fn mapped_markdown_has_width_independent_plain_text_and_exact_fragments() {
    let source =
        "## Heading\n\n**bold** &amp; `raw` 中🙂e\u{301}\n\n> quote\n\n```rs\n    let x = 1;\n```";
    let expected = "Heading\n\nbold & raw 中🙂e\u{301}\n\nquote\n\n    let x = 1;";
    for width in [8, 15, 40, 100] {
        let layout = markdown::render_layout(
            source,
            width,
            crate::math::MathPresentation::Native,
            markdown::Completion::Final,
        )
        .expect("layout");
        assert_eq!(layout.text, expected);
        assert_eq!(layout.rows.len(), layout.lines.len());
        for (row, fragments) in layout.rows.iter().enumerate() {
            for fragment in fragments {
                let text = &layout.text[fragment.text.clone()];
                let mut x = fragment.column;
                for (byte, grapheme) in text.grapheme_indices(true) {
                    for cell in 0..grapheme.width() {
                        assert_eq!(
                            layout.offset_at(row, x + cell),
                            Some(fragment.text.start + byte)
                        );
                    }
                    x += grapheme.width();
                }
            }
        }
    }
}

/// MD-2/SEL-2: table cell text is copied in row order, not with grid padding or soft wraps.
#[test]
fn mapped_tables_copy_cell_text_without_alignment_padding() {
    let source = "| Name | Value |\n| --- | ---: |\n| alpha beta gamma | 中文🙂 |\n| delta | 42 |";
    for width in [12, 30, 100] {
        let layout = markdown::render_layout(
            source,
            width,
            crate::math::MathPresentation::Native,
            markdown::Completion::Final,
        )
        .expect("table");
        assert_eq!(
            layout.text,
            "Name\tValue\nalpha beta gamma\t中文🙂\ndelta\t42"
        );
        for (row, fragments) in layout.rows.iter().enumerate() {
            for fragment in fragments {
                assert_eq!(
                    layout.offset_at(row, fragment.column),
                    Some(fragment.text.start)
                );
                assert!(layout.text.is_char_boundary(fragment.text.end));
            }
        }
    }
}

/// MD-4/MTH-1: cache checkpoints cannot slice through UTF-8 or an atomic formula rectangle.
#[test]
fn prefix_validation_rejects_utf8_and_formula_straddles_without_allocating() {
    let text = "é".to_owned();
    let utf8 = Layout {
        lines: vec![Line::default()],
        text,
        rows: vec![vec![Fragment {
            column: 0,
            text: 0..2,
            kind: FragmentKind::Text,
        }]],
        formulas: Vec::new(),
    };
    assert!(!utf8.prefix_valid(1, 1));
    assert!(utf8.prefix_valid(1, 2));

    let formula = math::PlacedFormula {
        column: 0,
        row: 0,
        width: 1,
        height: 2,
        text: 0..1,
        content: math::FormulaContent::Pending,
        style: Paint::default(),
    };
    let straddled = Layout {
        lines: vec![Line::default(), Line::default()],
        text: "x".into(),
        rows: vec![
            vec![Fragment {
                column: 0,
                text: 0..1,
                kind: FragmentKind::Atomic { columns: 1 },
            }],
            vec![Fragment {
                column: 0,
                text: 0..1,
                kind: FragmentKind::Atomic { columns: 1 },
            }],
        ],
        formulas: vec![formula],
    };
    assert!(!straddled.prefix_valid(1, 1));
}
