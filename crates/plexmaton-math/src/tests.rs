use super::*;

#[path = "test_support.rs"]
mod support;

fn prepared(source: &str) -> FormulaLayout {
    Formula::parse(source)
        .expect("admitted source")
        .layout(120)
        .expect("native layout")
}

/// MTH-1/MTH-3: every cell, including gaps and edges, owns exact delimited source.
#[test]
fn formula_hit_cells_are_atomic_and_preserve_original_delimiters() {
    for (open, close, mode) in [
        ("$", "$", MathMode::Inline),
        (r"\(", r"\)", MathMode::Inline),
        ("$$", "$$", MathMode::Display),
        (r"\[", r"\]", MathMode::Display),
    ] {
        let source = format!("{open}\n \\frac{{ab}}{{c}} \r\n{close}");
        let formula = Formula::parse(&source).expect("complete formula");
        assert_eq!(formula.mode(), mode);
        for width in [120, 88, 60] {
            let layout = formula.layout(width).expect("layout");
            assert_eq!(layout.source(), source);
            assert!(
                layout
                    .runs()
                    .iter()
                    .map(|run| usize::from(run.rows) * usize::from(run.columns))
                    .sum::<usize>()
                    < usize::from(layout.width()) * usize::from(layout.height()),
                "fixture must exercise blank hit cells"
            );
            for row in 0..usize::from(layout.height()) {
                for column in 0..usize::from(layout.width()) {
                    assert_eq!(layout.source_at(column, row), Some(source.as_str()));
                }
            }
            assert_eq!(layout.source_at(usize::from(layout.width()), 0), None);
            assert_eq!(layout.source_at(0, usize::from(layout.height())), None);
        }
    }
}

/// MTH-2/MTH-3: no occurrence in the complete reply is skipped or silently simplified.
#[test]
fn complete_attention_reply_preserves_all_formula_occurrences_at_three_widths() {
    let fixture = support::reply();
    assert_eq!(fixture.math.len(), 61);
    assert_eq!(fixture.math.iter().filter(|span| span.display).count(), 35);
    let mut previous = 0;
    for (index, span) in fixture.math.iter().enumerate() {
        assert!(span.start >= previous);
        previous = span.end;
        let source = &fixture.text[span.start..span.end];
        let formula = Formula::parse(source)
            .unwrap_or_else(|error| panic!("formula {index}: {error}: {source}"));
        assert_eq!(formula.mode() == MathMode::Display, span.display);
        for width in [120, 88, 60] {
            let layout = formula
                .layout(width)
                .unwrap_or_else(|error| panic!("formula {index} at {width}: {error}: {source}"));
            assert!(!layout.runs().is_empty());
            assert!(usize::from(layout.width()) <= width);
            assert_eq!(layout.source(), source);
            assert_disjoint(&layout);
            assert_eq!(layout.runs(), formula.layout(width).expect("repeat").runs());
        }
    }
}

fn assert_disjoint(layout: &FormulaLayout) {
    let mut cells = std::collections::BTreeSet::new();
    for run in layout.runs() {
        assert!(!run.text.chars().any(char::is_control));
        assert!(run.columns > 0 && run.rows > 0);
        for row in run.y..run.y + run.rows {
            for column in run.x..run.x + run.columns {
                assert!(column < layout.width() && row < layout.height());
                assert!(cells.insert((column, row)), "overlap at {column},{row}");
            }
        }
    }
}

/// MTH-2: fraction rows and paired scripts cannot collapse during quantization.
#[test]
fn fraction_rows_and_paired_scripts_retain_engine_geometry() {
    let layout = prepared(r"\[\frac{a}{b}\]");
    let row = |text| {
        layout
            .runs()
            .iter()
            .find(|run| run.text == text)
            .expect("glyph")
            .y
    };
    assert!(row("a") < row("─"));
    assert!(row("─") < row("b"));
    let layout = prepared(r"\[x_{ij}^{n+1}\]");
    let base = layout
        .runs()
        .iter()
        .find(|run| run.text == "x")
        .expect("base");
    let lower = layout
        .runs()
        .iter()
        .find(|run| run.text == "ij")
        .expect("subscript");
    let upper = layout
        .runs()
        .iter()
        .find(|run| run.text.contains('n'))
        .expect("superscript");
    assert_eq!(lower.scale, TextScale::Script);
    assert_eq!(upper.scale, TextScale::Script);
    assert!(upper.y <= base.y && lower.y >= base.y && upper.y < lower.y);
    assert_disjoint(&layout);
}

/// MTH-2: font glyph codes and accents retain their mathematical meaning.
#[test]
fn font_glyph_mapping_preserves_not_equal_double_struck_and_macron() {
    let layout = prepared(r"\[j\neq k+\mathbb{R}+\bar a+\mathbf 1\]");
    let text: String = layout.runs().iter().map(|run| run.text.as_str()).collect();
    assert!(
        text.contains('≠') && text.contains('ℝ') && text.contains("a\u{304}"),
        "{text}"
    );
    assert!(
        layout
            .runs()
            .iter()
            .any(|run| run.text == "1" && run.style == FontStyle::Bold)
    );
}

/// MTH-2: inheritable paint differs from explicit black and cannot be forged by source.
#[test]
fn explicit_colors_do_not_become_palette_inheritance() {
    assert!(
        prepared("$x$")
            .runs()
            .iter()
            .all(|run| run.paint == Paint::Inherit)
    );
    assert!(
        prepared(r"$\textcolor{black}{x}$")
            .runs()
            .iter()
            .all(|run| run.paint
                == Paint::Rgb {
                    red: 0,
                    green: 0,
                    blue: 0
                })
    );
    assert!(Formula::parse(r"$\textcolor{rgb(-255,-255,-255)}{x}$").is_err());
}

/// MTH-4: invalid input and width overflow are refusals, never successful truncation.
#[test]
fn source_and_native_limits_refuse_without_truncation_or_macro_leakage() {
    for source in ["x", "$x", r"\[x\)", "$$x$"] {
        assert!(
            matches!(Formula::parse(source), Err(MathError::Delimiters)),
            "{source}"
        );
    }
    assert!(matches!(
        Formula::parse("$x\u{1b}[2J$"),
        Err(MathError::Controls)
    ));
    assert!(matches!(
        Formula::parse(&format!("${}$", "x".repeat(MAX_SOURCE_BYTES))),
        Err(MathError::Limited(Limit::SourceBytes))
    ));
    assert!(matches!(
        Formula::parse(r"$\frac{x}{$"),
        Err(MathError::ParseRejected)
    ));
    assert!(matches!(
        Formula::parse(r"$\ownedmacro$"),
        Err(MathError::ParseRejected)
    ));
    Formula::parse(r"$\gdef\ownedmacro{x}\ownedmacro$").expect("local macro");
    assert!(matches!(
        Formula::parse(r"$\ownedmacro$"),
        Err(MathError::ParseRejected)
    ));
    assert!(matches!(
        Formula::parse(&format!("${}$", "x".repeat(MAX_NODES + 1))),
        Err(MathError::Limited(Limit::Nodes))
    ));
    assert!(matches!(
        Formula::parse(&format!("${}x{}$", "{".repeat(40), "}".repeat(40))),
        Err(MathError::ParseRejected)
    ));
    let formula = Formula::parse(r"$abcdefgh$").expect("parse");
    assert!(matches!(formula.layout(4), Err(MathError::TooWide { .. })));
    assert!(matches!(
        formula.layout(0),
        Err(MathError::Limited(Limit::Geometry))
    ));
    assert!(matches!(
        formula.layout(MAX_DIMENSION + 1),
        Err(MathError::Limited(Limit::Geometry))
    ));
    assert_eq!(formula.source(), "$abcdefgh$");
}

/// MTH-2: supported structural fixtures render; narrow indivisible overflow is explicit.
#[test]
fn structural_corpus_preserves_tables_roots_and_explicit_overflow() {
    for (name, body) in support::CORPUS {
        let source = format!("\\[{body}\\]");
        if *name == "incomplete stream" {
            assert!(matches!(
                Formula::parse(&source),
                Err(MathError::ParseRejected)
            ));
            continue;
        }
        let formula = Formula::parse(&source).expect(name);
        for width in [120, 88, 60] {
            if *name == "wide" && width < 91 {
                assert!(matches!(
                    formula.layout(width),
                    Err(MathError::TooWide { available, .. }) if available == width
                ));
            } else {
                let layout = formula.layout(width).expect(name);
                assert_disjoint(&layout);
                if *name == "matrix" {
                    for (top, bottom) in [("⎛", "⎝"), ("⎞", "⎠")] {
                        let top = layout
                            .runs()
                            .iter()
                            .find(|run| run.text == top)
                            .expect("top");
                        let bottom = layout
                            .runs()
                            .iter()
                            .find(|run| run.text == bottom)
                            .expect("bottom");
                        assert_eq!(top.x, bottom.x);
                        assert!(top.y < bottom.y);
                    }
                }
            }
        }
    }
}

/// MTH-2/MTH-4: collisions are refused before native output can erase another reservation.
#[test]
fn independent_native_overprint_is_refused() {
    let formula = Formula::parse("$x$").expect("parse");
    let scene = &formula.0.scene;
    let duplicated = engine::Scene {
        width: scene.width,
        height: scene.height,
        axis: scene.axis,
        items: vec![scene.items[0].clone(), scene.items[0].clone()],
    };
    assert!(matches!(
        native::project(&duplicated, 60),
        Err(MathError::Overlap)
    ));
    let mut rule_collision = duplicated;
    rule_collision.items[1].kind = engine::Kind::Vertical;
    rule_collision.items[1].top = 0.0;
    rule_collision.items[1].bottom = 2.0;
    assert!(matches!(
        native::project(&rule_collision, 60),
        Err(MathError::Overlap)
    ));
}

/// MTH-4: cell allocation is refused even when both individual dimensions are valid.
#[test]
fn aggregate_cell_bound_is_checked_before_paint_allocation() {
    let horizontal = engine::Item {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        top: 0.0,
        bottom: 0.04,
        kind: engine::Kind::Horizontal,
        paint: Paint::Inherit,
    };
    let vertical = engine::Item {
        x: 99.0,
        y: 0.0,
        width: 0.04,
        top: 0.0,
        bottom: 400.0,
        kind: engine::Kind::Vertical,
        paint: Paint::Inherit,
    };
    let scene = engine::Scene {
        width: 100.0,
        height: 400.0,
        axis: 0.0,
        items: vec![horizontal, vertical],
    };
    assert!(matches!(
        native::project(&scene, MAX_DIMENSION),
        Err(MathError::Limited(Limit::Cells))
    ));
}

/// MTH-2: default frames follow foreground, while an explicit source color stays explicit.
#[test]
fn framed_paint_inherits_without_erasing_explicit_color() {
    assert!(
        prepared(r"$\boxed{x}$")
            .runs()
            .iter()
            .all(|run| run.paint == Paint::Inherit)
    );
    for (source, expected) in [
        (
            r"$\textcolor{red}{\boxed{x}}$",
            Paint::Rgb {
                red: 255,
                green: 0,
                blue: 0,
            },
        ),
        (
            r"$\textcolor{black}{\boxed{x}}$",
            Paint::Rgb {
                red: 0,
                green: 0,
                blue: 0,
            },
        ),
    ] {
        assert!(
            prepared(source)
                .runs()
                .iter()
                .all(|run| run.paint == expected)
        );
    }
    assert!(matches!(
        Formula::parse(r"$\textcolor{#0000}{x}$"),
        Err(MathError::Unsupported(Unsupported::Paint))
    ));
    assert!(Formula::parse(r"$\textcolor[rgb]{-1,-1,-1}{x}$").is_err());
}

/// MTH-2: root pieces span the body, and engine word gaps do not disappear in the cell grid.
#[test]
fn radicals_span_the_radicand_and_text_keeps_word_gaps() {
    let layout = prepared(r"$\sqrt{d_k}$");
    let root = layout
        .runs()
        .iter()
        .find(|run| run.text == "√")
        .expect("radical foot");
    let top = layout
        .runs()
        .iter()
        .find(|run| run.text == "┌")
        .expect("radical top");
    let body = layout
        .runs()
        .iter()
        .find(|run| run.text == "d")
        .expect("radicand");
    assert_eq!(root.x, top.x);
    assert!(top.y < body.y && root.y >= body.y);
    let layout = prepared(r"$\text{number of heads}$");
    let words: Vec<_> = layout.runs().iter().collect();
    assert_eq!(
        words
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>(),
        ["number", "of", "heads"]
    );
    for pair in words.windows(2) {
        assert!(pair[1].x > pair[0].x + pair[0].columns);
    }
}

/// MTH-1/MTH-2: the user's derivative and probability vector keep the circumflex on p,
/// all numeric terms and exact delimited source at every supported review width.
#[test]
fn logits_accents_preserve_prediction_and_gradient_at_three_widths() {
    let fixture = support::logits_reply();
    for (index, span) in fixture.math[..2].iter().enumerate() {
        let source = &fixture.text[span.start..span.end];
        let formula = Formula::parse(source).expect("logits formula");
        for width in [120, 88, 60] {
            let layout = formula.layout(width).expect("native logits layout");
            let text: String = layout.runs().iter().map(|run| run.text.as_str()).collect();
            assert!(text.contains("p\u{0302}"), "circumflex lost: {text}");
            let expected = if index == 0 {
                &["∂", "L", "logits", "=", "−", "y"][..]
            } else {
                &["=", "[", "0.7", "0.2", "0.1", "]"][..]
            };
            for term in expected {
                assert!(text.contains(term), "lost {term:?}: {text}");
            }
            assert_eq!(layout.source(), source);
            assert_disjoint(&layout);
        }
    }
}

/// MTH-1/MTH-2: CJK labels, arrows and the surrounding box retain their meaning and two-cell
/// glyph reservations; a terminal font supplies the glyphs without replacing their source.
#[test]
fn logits_cjk_labels_preserve_all_text_and_box_at_three_widths() {
    let fixture = support::logits_reply();
    for (index, span) in fixture.math[2..].iter().enumerate() {
        let source = &fixture.text[span.start..span.end];
        let formula = Formula::parse(source).expect("mixed-language formula");
        for width in [120, 88, 60] {
            let layout = formula.layout(width).expect("native mixed-language layout");
            let text: String = layout.runs().iter().map(|run| run.text.as_str()).collect();
            let expected = if index == 0 {
                &["logits", "softmax", "概率", "和真实标签做", "cross-entropy"][..]
            } else {
                &["真实标签", "y", "对比", "模型预测概率", "p\u{0302}"][..]
            };
            for term in expected {
                assert!(text.contains(term), "lost {term:?}: {text}");
            }
            if index == 0 {
                assert_eq!(text.matches('→').count(), 2);
            } else {
                for corner in ['┌', '┐', '└', '┘'] {
                    assert!(text.contains(corner), "missing box corner {corner}: {text}");
                }
            }
            assert_eq!(layout.source(), source);
            assert_disjoint(&layout);
        }
    }
}

/// MTH-2: admitted CJK scripts and single-base hats keep scale, Unicode and paint; an accent
/// with a different color cannot silently inherit its base's foreground.
#[test]
fn cjk_scripts_and_single_base_accents_keep_unicode_scale_and_paint() {
    for (source, expected) in [
        (
            r"\[\text{概率 カナ 한글 ＡＢ}\]",
            &["概率", "カナ", "한글", "ＡＢ"][..],
        ),
        (r"$\frac{\text{概率}}{\text{标签}}$", &["概率", "标签"][..]),
        (r"\[\hat q + \hat\alpha\]", &["q\u{0302}", "α\u{0302}"][..]),
    ] {
        let layout = prepared(source);
        let text: String = layout.runs().iter().map(|run| run.text.as_str()).collect();
        for term in expected {
            assert!(text.contains(term), "lost {term}: {text}");
        }
        assert_disjoint(&layout);
        if source.starts_with('$') {
            assert!(
                layout
                    .runs()
                    .iter()
                    .filter(|run| run.text.contains('概') || run.text.contains('标'))
                    .all(|run| run.scale == TextScale::Script && run.columns >= 3)
            );
        }
    }
    let formula = Formula::parse(r"\[\hat{\textcolor{blue}{p}}\]").expect("valid colored accent");
    assert!(matches!(formula.layout(120), Err(MathError::Overlap)));
}
