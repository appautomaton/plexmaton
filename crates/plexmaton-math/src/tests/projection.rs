use super::*;

/// MTH-1/MTH-2/MTH-3: the reported roots and multiline loss retain source and every native atom.
#[test]
fn reported_roots_and_log_sum_exp_loss_project_at_three_widths() {
    let fixture: support::Reply =
        serde_json::from_str(include_str!("../../fixtures/projection.json"))
            .expect("source-linked projection cases");
    assert_eq!(fixture.math.len(), 4);
    for (index, span) in fixture.math.iter().enumerate() {
        let source = &fixture.text[span.start..span.end];
        let formula = Formula::parse(source)
            .unwrap_or_else(|error| panic!("case {index}: {error:?}: {source}"));
        for width in [120, 88, 60] {
            let layout = formula
                .layout(width)
                .unwrap_or_else(|error| panic!("case {index} at {width}: {error:?}"));
            assert_eq!(layout.source(), source);
            assert_disjoint(&layout);
            for row in 0..usize::from(layout.height()) {
                for column in 0..usize::from(layout.width()) {
                    assert_eq!(layout.source_at(column, row), Some(source));
                }
            }
            if index < 3 {
                assert_joined_radical(&layout);
            }
            if index == 3 {
                let text: String = layout.runs().iter().map(|run| run.text.as_str()).collect();
                assert_eq!(text.chars().filter(|ch| *ch == '∑').count(), 2);
                for term in [
                    "L", "y", "z", "log", "e", "i", "j", "⎛", "⎜", "⎝", "⎞", "⎟", "⎠",
                ] {
                    assert!(text.contains(term), "lost {term}: {text}");
                }
            }
        }
    }
}

fn assert_joined_radical(layout: &FormulaLayout) {
    let root = layout
        .runs()
        .iter()
        .find(|run| run.text == "√")
        .expect("radical glyph");
    let roof = layout
        .runs()
        .iter()
        .filter(|run| run.text == "─" && run.y == root.y)
        .min_by_key(|run| run.x)
        .expect("radical roof");
    assert_eq!(root.scale, TextScale::Large);
    assert_eq!((root.columns, root.rows), (2, 2));
    assert_eq!(roof.x, root.x + root.columns);
}

/// MTH-2/MTH-3: larger or indexed roots retain their stem, roof and index without overprint.
#[test]
fn tall_and_indexed_roots_keep_bounded_nonoverlapping_geometry() {
    for (source, indexed) in [
        (r"\[\sqrt{\frac{a}{b}}\]", false),
        (r"\[\sqrt[3]{\frac{a}{b}}\]", true),
        (r"\[\sqrt[3]{x}\]", true),
    ] {
        let formula = Formula::parse(source).expect("admitted root");
        for width in [120, 88, 60] {
            let layout = formula.layout(width).expect("bounded root layout");
            assert_eq!(layout.source(), source);
            assert_disjoint(&layout);
            let top = layout
                .runs()
                .iter()
                .find(|run| run.text == "┌" || (run.text == "√" && run.scale == TextScale::Large))
                .expect("root top");
            let roof = layout
                .runs()
                .iter()
                .find(|run| run.text == "─" && run.y == top.y)
                .expect("contiguous root roof");
            assert_eq!(roof.x, top.x + top.columns);
            if indexed {
                let index = layout
                    .runs()
                    .iter()
                    .find(|run| run.text == "3")
                    .expect("root index");
                let body = layout
                    .runs()
                    .iter()
                    .find(|run| run.text == "x" || run.text == "a")
                    .expect("radicand");
                assert!(index.x + index.columns <= top.x);
                assert!(index.y <= body.y);
            }
        }
    }
}

/// MTH-2/MTH-4: matching parser structure never admits another path producer by resemblance.
#[test]
fn non_parenthesis_vector_paths_remain_a_typed_refusal() {
    assert!(matches!(
        Formula::parse(r"\[\left|\sum_i y_i\right|\]"),
        Err(MathError::Unsupported(Unsupported::Path))
    ));
}

/// MTH-2: a numerator's nested script cannot become an unrelated denominator root index.
#[test]
fn unrelated_nested_scripts_stay_in_the_numerator() {
    for numerator in [r"x_{a_b}", r"x^{a^b}"] {
        let source = format!(r"\[\frac{{{numerator}}}{{x+\sqrt{{x}}}}\]");
        for width in [120, 88, 60] {
            let layout = Formula::parse(&source)
                .expect("fraction")
                .layout(width)
                .expect("layout");
            let script = layout
                .runs()
                .iter()
                .find(|r| r.text == "b")
                .expect("nested script");
            let bar_y = layout
                .runs()
                .iter()
                .filter(|run| run.text == "─")
                .map(|run| run.y)
                .min()
                .expect("fraction bar");
            assert!(
                script.y < bar_y,
                "nested script moved below numerator: {:?}",
                layout.runs()
            );
            assert_disjoint(&layout);
        }
    }
}

/// MTH-2: an owned compound index preserves its terms and spacing beside the radical.
#[test]
fn compound_root_indices_keep_their_complete_group() {
    for index in ["n+1", "12", "a-b"] {
        let source = format!(r"\[\sqrt[{index}]{{x}}\]");
        for width in [120, 88, 60] {
            let layout = Formula::parse(&source)
                .expect("indexed root")
                .layout(width)
                .expect("index layout");
            let root = layout
                .runs()
                .iter()
                .find(|r| r.text == "√")
                .expect("radical");
            let indices: Vec<_> = layout
                .runs()
                .iter()
                .filter(|r| r.scale == TextScale::ScriptScript)
                .collect();
            assert_eq!(
                indices.iter().map(|r| r.text.as_str()).collect::<String>(),
                index.replace('-', "−")
            );
            assert!(indices.iter().all(|r| r.x + r.columns <= root.x));
            assert_disjoint(&layout);
        }
    }
}

/// MTH-2: a root in a script cannot be promoted to the full-size large radical primitive.
#[test]
fn script_roots_do_not_use_full_size_radicals() {
    for source in [r"\[x^{\sqrt{y}}\]", r"\[x_{\sqrt{y}}\]"] {
        let layout = Formula::parse(source)
            .expect("script root")
            .layout(60)
            .expect("layout");
        let root = layout
            .runs()
            .iter()
            .find(|run| run.text == "√")
            .expect("radical");
        assert_ne!(root.scale, TextScale::Large);
        assert_disjoint(&layout);
    }
}
