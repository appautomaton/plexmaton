use super::*;

#[derive(Deserialize)]
struct Reply {
    text: String,
    math: Vec<SourceSpan>,
}

#[derive(Deserialize)]
struct SourceSpan {
    start: usize,
    end: usize,
}

/// MTH-1/MTH-2/PRE-1: the production Markdown path retains every occurrence, including in lists.
#[test]
fn complete_reply_composes_native_math_and_exact_atomic_maps_at_three_widths() {
    let reply: Reply = serde_json::from_str(include_str!(
        "../../../../plexmaton-math/fixtures/attention-derivatives.json"
    ))
    .expect("source-linked corpus");
    let mut canonical = None;
    for width in [114, 82, 54] {
        let layout = crate::markdown::render_layout(&reply.text, width, MathPresentation::Native)
            .expect("complete reply");
        assert_eq!(layout.formulas.len(), 61, "all occurrences at {width}");
        for (index, (formula, source)) in layout.formulas.iter().zip(&reply.math).enumerate() {
            assert!(
                matches!(formula.content, FormulaContent::Native(_)),
                "formula {index} at {width}: {:?}",
                formula.content
            );
            assert_eq!(
                &layout.text[formula.text.clone()],
                &reply.text[source.start..source.end],
                "occurrence {index}"
            );
            assert!(formula.column + formula.width <= width);
            assert!(formula.row + formula.height <= layout.lines.len());
            for row in formula.row..formula.row + formula.height {
                for column in formula.column..formula.column + formula.width {
                    assert_eq!(
                        layout.atom_at(row, column),
                        Some(formula.text.clone()),
                        "{index}, {row},{column}"
                    );
                }
            }
        }
        assert!(layout.lines.iter().all(|line| line.width() <= width));
        if let Some(canonical) = &canonical {
            assert_eq!(&layout.text, canonical, "width-independent semantic copy");
        }
        canonical = Some(layout.text.clone());
        let encoded = serde_json::to_vec(&layout).expect("worker reply");
        let decoded: Layout = serde_json::from_slice(&encoded).expect("admitted native reply");
        assert_eq!(decoded.text, layout.text);
        assert_eq!(decoded.formulas.len(), 61);
    }
}

/// MTH-1/MD-4: refusing one formula preserves surrounding prose and its exact source, at every width.
#[test]
fn formula_failures_are_local_typed_and_keep_source_copy_independent_of_capability() {
    let source = "before **bold** \\(x_{ij}^2\\) middle \\[\\frac{a}{b}\\] after";
    let mut canonical = None;
    for width in [120, 88, 60] {
        for math in [
            MathPresentation::Native,
            MathPresentation::Source(MathUnavailable::Unverified),
            MathPresentation::Source(MathUnavailable::Unsupported),
            MathPresentation::Source(MathUnavailable::Multiplexer),
        ] {
            let layout = crate::markdown::render_layout(source, width, math)
                .expect("local formula presentation");
            assert_eq!(layout.formulas.len(), 2);
            assert!(!layout.text.contains("Math source"));
            if let Some(canonical) = &canonical {
                assert_eq!(&layout.text, canonical);
            }
            canonical = Some(layout.text);
        }
    }
    for (source, expected, copied) in [
        (
            r"before \(\unknowncommand{x}\) after",
            SourceReason::Syntax,
            r"before \(\unknowncommand{x}\) after",
        ),
        (
            r"before \[\frac{a}{",
            SourceReason::Incomplete,
            "before \n\\[\\frac{a}{",
        ),
    ] {
        let layout = crate::markdown::render_layout(source, 60, MathPresentation::Native)
            .expect("local refusal");
        assert_eq!(layout.formulas.len(), 1);
        assert_eq!(layout.formulas[0].content, FormulaContent::Source(expected));
        assert_eq!(layout.text, copied);
    }
}

/// MTH-1/SEL-2: even a one-byte intersection paints the entire formula rectangle, not just glyphs.
#[test]
fn an_atomic_range_highlights_every_blank_and_edge_cell() {
    let palette = crate::Palette::ansi();
    let selection = palette.style(Role::Selection);
    for math in [MathPresentation::Native, MathPresentation::default()] {
        let layout = crate::markdown::render_layout(r"before \(\frac{a}{b}\) after", 60, math)
            .expect("formula");
        let formula = &layout.formulas[0];
        let lines = layout.highlighted_lines(
            formula.text.start..formula.text.start + 1,
            &palette,
            selection,
        );
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(
            0,
            0,
            60,
            u16::try_from(lines.len()).expect("rows"),
        ));
        use ratatui::widgets::Widget as _;
        let mut before = buffer.clone();
        ratatui::widgets::Paragraph::new(layout.painted_lines(&palette))
            .render(before.area, &mut before);
        ratatui::widgets::Paragraph::new(lines).render(buffer.area, &mut buffer);
        for row in formula.row..formula.row + formula.height {
            for column in formula.column..formula.column + formula.width {
                let cell = &buffer[(
                    u16::try_from(column).expect("column"),
                    u16::try_from(row).expect("row"),
                )];
                let mut expected = before[(
                    u16::try_from(column).expect("column"),
                    u16::try_from(row).expect("row"),
                )]
                    .clone();
                expected.set_style(selection);
                assert_eq!(cell, &expected, "{math:?} at {column},{row}");
            }
        }
    }
}

/// MTH-1/SEL-2/MD-2: table cells keep native boxes and row-major copy in both grid and labelled views.
#[test]
fn native_table_cells_keep_atomic_geometry_and_exact_tabular_copy_when_narrow() {
    let source = "| Parameter with a descriptive name | Equation | Interpretation |\n| --- | ---: | --- |\n| first | \\( \\frac{ab}{c} \\) | **ready** |\n| second | $x_{ij}^2$ | after |";
    let expected = "Parameter with a descriptive name\tEquation\tInterpretation\nfirst\t\\( \\frac{ab}{c} \\)\tready\nsecond\t$x_{ij}^2$\tafter";
    for width in [114, 82, 54, 24] {
        for math in [MathPresentation::Native, MathPresentation::default()] {
            let layout = crate::markdown::render_layout(source, width, math).expect("native table");
            assert_eq!(layout.text, expected, "{width}, {math:?}");
            assert_eq!(layout.formulas.len(), 2);
            assert!(layout.formulas_validate(width));
            assert!(layout.lines.iter().all(|line| line.width() <= width));
            for formula in &layout.formulas {
                assert_eq!(
                    matches!(formula.content, FormulaContent::Native(_)),
                    math == MathPresentation::Native
                );
                for row in formula.row..formula.row + formula.height {
                    assert_eq!(
                        layout.atom_at(row, formula.column + formula.width - 1),
                        Some(formula.text.clone())
                    );
                }
            }
        }
    }
    let source = "| $x_i$ | A descriptive heading |\n| --- | --- |\n| one | value |";
    let layout =
        crate::markdown::render_layout(source, 24, MathPresentation::Native).expect("math header");
    assert_eq!(layout.text, "$x_i$\tA descriptive heading\none\tvalue");
    assert_eq!(
        layout.formulas.len(),
        1,
        "a mathematical header is drawn once, not repeated as an uncopyable label"
    );
    assert!(layout.formulas_validate(24));
}

/// MTH-2/PRE-1: an indivisible oversized script is a local refusal, not a corrupt worker reply.
#[test]
fn native_transport_limits_refuse_locally_before_a_prepared_reply_is_encoded() {
    let source = r"before \(x_{\text{aaaaaaaaaaaaaaaaaaaa}}\) after";
    let layout = crate::markdown::render_layout(source, 60, MathPresentation::Native)
        .expect("local refusal");
    assert_eq!(layout.text, source);
    assert_eq!(layout.formulas.len(), 1);
    assert_eq!(
        layout.formulas[0].content,
        FormulaContent::Source(SourceReason::Unsupported)
    );
    assert!(layout.formulas_validate(60));
    let encoded = serde_json::to_vec(&layout).expect("reply");
    let decoded: Layout =
        serde_json::from_slice(&encoded).expect("valid reply despite one unsupported formula");
    assert_eq!(decoded.text, source);
    assert!(
        crate::markdown::render_layout(
            &"$x$ ".repeat(MAX_FORMULAS + 1),
            60,
            MathPresentation::Native
        )
        .is_err(),
        "formula count is bounded during preparation"
    );
}
