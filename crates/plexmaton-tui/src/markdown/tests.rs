use super::*;

fn text(lines: &[ratatui::text::Line<'_>]) -> String {
    lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// MD-1/MD-2/MD-5: changing colors preserves every logical byte, wrapped row and copy fragment.
#[test]
fn markdown_pastel_changes_only_style_and_keeps_nested_modifiers() {
    let source = "# Blue heading\n\n## Green heading\n\n### Lavender heading\n\n**bold `command` and [link](https://example.com)**\n\n> A gentle quote.\n\n- 中文 e\u{301}\n\n```rust\n    let x = \"literal **text**\";\n```\n\n| Name | Value |\n| --- | --- |\n| result | **ready** |";
    let base = Palette::ansi();
    let proposed = base.with_markdown_theme(crate::MarkdownTheme::Pastel);
    for width in [12, 60, 88, 120] {
        let prepared = render_layout(source, width, MathPresentation::Native, Completion::Final)
            .expect("prepared");
        let before = prepared.painted_lines(&base);
        let after = prepared.painted_lines(&proposed);
        assert_eq!(text(&before), text(&after), "wrapping at {width}");
    }
    let lines = render(source, &proposed, 120).expect("styled");
    let style = |content| {
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .find(|span| span.content == content)
            .expect("fixture span")
            .style
    };
    let designed = Palette::pastel().markdown_styles();
    assert_eq!(style("Blue heading").fg, designed.headings[0].fg);
    assert_eq!(style("Green heading").fg, designed.headings[1].fg);
    assert_eq!(style("Lavender heading").fg, designed.headings[2].fg);
    assert!(style("command").add_modifier.contains(Modifier::BOLD));
    assert!(
        style("link")
            .add_modifier
            .contains(Modifier::BOLD | Modifier::UNDERLINED)
    );
    assert!(
        style("A gentle quote.")
            .add_modifier
            .contains(Modifier::ITALIC)
    );
    let monochrome = render(source, &Palette::monochrome(), 120).expect("monochrome");
    assert!(
        monochrome
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.fg.is_none() && span.style.bg.is_none())
    );
}

/// MD-4: plain prose avoids parsing; every supported syntax trigger still reaches CommonMark.
#[test]
fn markdown_admission_keeps_plain_history_on_the_lightweight_path() {
    for source in [
        "Hello world.",
        "普通文字，不需要额外排版。",
        "The result is ready — 42 tokens.",
    ] {
        assert!(!may_format(source));
    }
    for source in [
        "# heading",
        "**bold**",
        "_italic_",
        "`code`",
        "[x](url)",
        "- item",
        "1. item",
        "+ item",
        "    code",
        "a\nb",
        "---",
        "> quote",
        "| table |",
        "~~gone~~",
        "&amp;",
        "\\*literal",
        "<b>html</b>",
        "a\u{1b}b",
    ] {
        assert!(may_format(source), "{source:?}");
    }
}

/// MD-1/MD-2: inline styles and block structure interpret Markdown without pretending code is prose.
#[test]
fn markdown_styles_blocks_and_keeps_code_literal() {
    let source = "## Result\n\nA **strong** and *gentle* `value`.\n\n- first\n- second\n\n> quoted\n\n```rust\n    let x = \"**literal**\";\n\tcall();\n```\n\n[docs](https://example.com)";
    let palette = Palette::pastel();
    let rows = render(source, &palette, 60).expect("render");
    let shown = text(&rows);
    assert!(shown.starts_with("Result\n"));
    assert!(shown.contains("• first\n• second"), "{shown}");
    assert!(shown.contains("│ quoted"));
    assert!(shown.contains("│     let x = \"**literal**\";"), "{shown}");
    assert!(shown.contains("│     call();"));
    assert!(shown.contains("docs (https://example.com)"));
    assert!(!shown.contains("**strong**"));
    assert!(
        rows.iter()
            .flat_map(|line| &line.spans)
            .any(|span| span.content.contains("strong")
                && span.style.add_modifier.contains(Modifier::BOLD))
    );
    assert!(
        rows.iter()
            .flat_map(|line| &line.spans)
            .any(|span| span.content.contains("gentle")
                && span.style.add_modifier.contains(Modifier::ITALIC))
    );
    assert!(source.contains("**strong**"));
}

/// MD-2: wide tables align cells; narrow tables retain each labelled value, including Unicode.
#[test]
fn markdown_tables_keep_all_values_at_wide_and_narrow_widths() {
    let source = "| Item | State | Detail |\n| :--- | ---: | :---: |\n| alpha | ready | 中文结果 |\n| beta | waiting | keep this value |";
    for width in [16, 40, 80] {
        let rows = render(source, &Palette::pastel(), width).expect("table");
        assert!(
            rows.iter().all(|line| line.width() <= width),
            "{width}: {}",
            text(&rows)
        );
        let shown = text(&rows);
        for value in ["alpha", "ready", "中文结果", "beta", "waiting"] {
            assert!(shown.contains(value), "{width}: {shown}");
        }
        if width == 16 {
            assert!(
                shown.contains("Item: alpha") && shown.contains("State: ready"),
                "{shown}"
            );
        } else {
            assert!(shown.contains('┼'));
        }
    }
}

/// MD-2/MD-3: every UTF-8 streaming prefix, including open fences, stays within its cell budget.
#[test]
fn markdown_streaming_prefixes_and_unicode_never_overflow() {
    let source =
        "# 你好\n\n**hello 👩‍💻 e\u{301}**\n\n1. one\n   - nested\n\n```rs\n  let a = 42;\n```";
    for end in source.char_indices().map(|(i, _)| i).chain([source.len()]) {
        for width in [12, 45] {
            let rows = render(&source[..end], &Palette::monochrome(), width)
                .unwrap_or_else(|reason| panic!("prefix {end}, width {width}: {reason:?}"));
            assert!(
                rows.iter().all(|line| line.width() <= width),
                "prefix {end}, width {width}: {}",
                text(&rows)
            );
        }
    }
    let open =
        render("```rust\n    let x = **literal**;", &Palette::ansi(), 45).expect("open fence");
    assert!(text(&open).contains("│     let x = **literal**;"));
    for width in [1, 4] {
        let rows = render("**你好 👩‍💻 e\u{301}**", &Palette::monochrome(), width)
            .expect("wide graphemes are replaced, not rejected");
        assert!(rows.iter().all(|line| line.width() <= width));
    }
    // MD-3: structural prefixes that consume the viewport explicitly fall back to source.
    assert_eq!(
        render("1. one", &Palette::monochrome(), 1),
        Err(PlainReason::Complexity)
    );
}

/// MD-1/MD-3: terminal controls are inert and excessive input/depth has a typed fallback.
#[test]
fn markdown_controls_and_limits_are_explicit() {
    let rows = render(
        "[click](https://example.com)\n\n<script>\u{1b}]52;c;payload\u{7}</script>",
        &Palette::ansi(),
        60,
    )
    .expect("inert");
    assert!(!text(&rows).contains('\u{1b}') && !text(&rows).contains('\u{7}'));
    assert!(text(&rows).contains("<script>"));
    assert_eq!(
        render(&"a".repeat(MAX_SOURCE_BYTES + 1), &Palette::ansi(), 60),
        Err(PlainReason::Size)
    );
    assert_eq!(
        render(&format!("{}text", "> ".repeat(40)), &Palette::ansi(), 120),
        Err(PlainReason::Complexity)
    );
}

/// MD-4/PRE-1: a completed heading is rendered once and its merged suffix matches one-shot rows.
#[test]
fn frozen_prefix_reuses_heading_and_keeps_full_layout_equivalent() {
    let source = "# Heading\n\nTail with **bold** and $x$";
    let full = render_layout_with_prefix(
        source,
        80,
        MathPresentation::Native,
        Completion::Streaming,
        None,
    )
    .expect("full preparation");
    let checkpoint = full.checkpoint.clone().expect("safe heading checkpoint");
    let prefix = full
        .layout
        .prefix(checkpoint.rows(), checkpoint.visible_text_bytes())
        .expect("row-aligned prefix");
    let hint = PrefixHint::new(checkpoint, prefix).expect("bounded hint");
    let reused = render_layout_with_prefix(
        source,
        80,
        MathPresentation::Native,
        Completion::Streaming,
        Some(&hint),
    )
    .expect("suffix preparation");
    assert!(reused.reused_prefix);
    assert_eq!(reused.layout.text, full.layout.text);
    assert_eq!(reused.layout.lines, full.layout.lines);
    assert_eq!(reused.layout.rows, full.layout.rows);
    assert_eq!(
        serde_json::to_vec(&reused.layout.formulas).expect("formula maps"),
        serde_json::to_vec(&full.layout.formulas).expect("formula maps")
    );
}

/// MD-4/PRE-1: suffix rendering retains reference targets resolved by the complete parser pass.
#[test]
fn frozen_prefix_uses_full_parser_events_for_late_reference_targets() {
    let initial = "[id]: /url\n\n# Heading\n\n";
    let full = render_layout_with_prefix(
        initial,
        80,
        MathPresentation::Native,
        Completion::Streaming,
        None,
    )
    .expect("full preparation");
    let checkpoint = full.checkpoint.clone().expect("heading checkpoint");
    let prefix = full
        .layout
        .prefix(checkpoint.rows(), checkpoint.visible_text_bytes())
        .expect("row-aligned prefix");
    let hint = PrefixHint::new(checkpoint, prefix).expect("bounded hint");
    let source = format!("{initial}[id]");
    let reused = render_layout_with_prefix(
        &source,
        80,
        MathPresentation::Native,
        Completion::Streaming,
        Some(&hint),
    )
    .expect("suffix preparation");
    let canonical = render_layout(&source, 80, MathPresentation::Native, Completion::Streaming)
        .expect("canonical preparation");
    assert!(reused.reused_prefix);
    assert_eq!(reused.layout, canonical);
    assert!(
        reused.layout.text.contains("(/url)"),
        "{}",
        reused.layout.text
    );
}

/// MD-4/MTH-1: frozen native formulas keep their atomic source ranges while only the tail is laid out.
#[test]
fn frozen_prefix_preserves_an_atomic_display_formula_and_copy_range() {
    let initial = "Intro\n\n$$x_i$$\n\n";
    let first = render_layout_with_prefix(
        initial,
        80,
        MathPresentation::Native,
        Completion::Streaming,
        None,
    )
    .expect("full preparation");
    assert_eq!(first.formula_preparations, 1);
    let checkpoint = first.checkpoint.clone().expect("formula checkpoint");
    let prefix = first
        .layout
        .prefix(checkpoint.rows(), checkpoint.visible_text_bytes())
        .expect("row-aligned prefix");
    let hint = PrefixHint::new(checkpoint, prefix).expect("bounded hint");
    let source = format!("{initial}Tail with 中文 and $y$");
    let reused = render_layout_with_prefix(
        &source,
        80,
        MathPresentation::Native,
        Completion::Streaming,
        Some(&hint),
    )
    .expect("suffix preparation");
    let canonical = render_layout_with_prefix(
        &source,
        80,
        MathPresentation::Native,
        Completion::Streaming,
        None,
    )
    .expect("canonical preparation");
    assert!(reused.reused_prefix);
    assert_eq!(
        reused.formula_preparations, 1,
        "only the mutable formula was prepared"
    );
    assert_eq!(canonical.formula_preparations, 2);
    assert_eq!(reused.layout, canonical.layout);
    let formula = reused.layout.formulas.first().expect("display formula");
    assert_eq!(&reused.layout.text[formula.text.clone()], "$$x_i$$");
    assert!(reused.layout.formulas_validate(80));
}

/// MD-4/PRE-3: a late reference definition invalidates the earlier event signature and falls back.
#[test]
fn frozen_prefix_invalidates_when_a_late_definition_changes_the_frozen_events() {
    let initial = "[id]\n\n# Heading\n\n";
    let first = render_layout_with_prefix(
        initial,
        80,
        MathPresentation::Native,
        Completion::Streaming,
        None,
    )
    .expect("full preparation");
    let checkpoint = first.checkpoint.clone().expect("heading checkpoint");
    let prefix = first
        .layout
        .prefix(checkpoint.rows(), checkpoint.visible_text_bytes())
        .expect("row-aligned prefix");
    let hint = PrefixHint::new(checkpoint, prefix).expect("bounded hint");
    let source = format!("{initial}[id]: /url");
    let prepared = render_layout_with_prefix(
        &source,
        80,
        MathPresentation::Native,
        Completion::Streaming,
        Some(&hint),
    )
    .expect("canonical fallback");
    assert!(!prepared.reused_prefix);
    let canonical = render_layout(&source, 80, MathPresentation::Native, Completion::Streaming)
        .expect("canonical preparation");
    assert_eq!(prepared.layout, canonical);
}

/// MD-4/MD-3: setext, containers and unfinished math cannot be used as a frozen boundary.
#[test]
fn frozen_prefix_rejects_spanning_or_global_markdown_state() {
    for source in [
        "Title\n===\n\nTail",
        "```text\ncode\n\nTail",
        "- one\n\nTail",
        "> quote\n\nTail",
        "| A | B |\n| - | - |\n| x | y |\n\nTail",
        "\\[x\n\nTail",
    ] {
        let prepared = render_layout_with_prefix(
            source,
            80,
            MathPresentation::Native,
            Completion::Streaming,
            None,
        )
        .expect("bounded source");
        assert!(
            prepared.checkpoint.is_none(),
            "unexpected checkpoint: {source:?}"
        );
        let canonical = render_layout(source, 80, MathPresentation::Native, Completion::Streaming)
            .expect("canonical fallback");
        assert_eq!(prepared.layout, canonical, "fallback changed {source:?}");
    }

    let crossing = render_layout_with_prefix(
        "# Heading\n\n```text\ncode\n\nTail",
        80,
        MathPresentation::Native,
        Completion::Streaming,
        None,
    )
    .expect("bounded open-fence source");
    assert_eq!(
        crossing
            .checkpoint
            .as_ref()
            .map(PrefixCheckpoint::source_prefix),
        Some("# Heading\n\n")
    );
}

/// MD-4/MD-1: append boundaries and suffix block kinds preserve canonical rows and copy maps.
#[test]
fn frozen_prefix_suffix_matrix_matches_canonical_at_three_widths() {
    for width in [120, 88, 60] {
        for initial in ["# Heading\n\n", "# Heading\n\n\\[x^2\\]\n\n"] {
            let first = render_layout_with_prefix(
                initial,
                width,
                MathPresentation::Native,
                Completion::Streaming,
                None,
            )
            .expect("initial render");
            let checkpoint = first.checkpoint.expect("complete prefix");
            let layout = first
                .layout
                .prefix(checkpoint.rows(), checkpoint.visible_text_bytes())
                .expect("complete prefix rows");
            let hint = PrefixHint::new(checkpoint, layout).expect("bounded prefix");
            for suffix in [
                "",
                "\n",
                "\nTail",
                "\n\nTail",
                "[id]: /url",
                "Tail\n\n[id]: /url",
                "中文 with **bold** and $y$",
                "> quote\n> continuation",
                "- one\n\n  continuation",
                "```text\ncode\n\nmore",
                "| A | B |\n| - | - |\n| x | y |",
                "\\[x\n\n",
            ] {
                let source = format!("{initial}{suffix}");
                let reused = render_layout_with_prefix(
                    &source,
                    width,
                    MathPresentation::Native,
                    Completion::Streaming,
                    Some(&hint),
                )
                .expect("bounded hinted render");
                let canonical = render_layout(
                    &source,
                    width,
                    MathPresentation::Native,
                    Completion::Streaming,
                )
                .expect("bounded canonical render");
                assert_eq!(reused.layout, canonical, "{width}: {source:?}");
            }
        }
    }
}

/// PRE-1/MD-4: an optional malformed copy map cannot turn valid source into unavailable text.
#[test]
fn frozen_prefix_rejects_malformed_hint_copy_ranges() {
    let first = render_layout_with_prefix(
        "# 中文\n\n",
        80,
        MathPresentation::Native,
        Completion::Streaming,
        None,
    )
    .expect("initial render");
    let checkpoint = first.checkpoint.expect("heading checkpoint");
    let layout = first
        .layout
        .prefix(checkpoint.rows(), checkpoint.visible_text_bytes())
        .expect("prefix rows");
    let hint = PrefixHint::new(checkpoint, layout).expect("bounded prefix");
    let source = "# 中文\n\nTail";
    for (pointer, value) in [
        ("/layout/rows/0/0/text/end", serde_json::json!(1)),
        ("/layout/rows/0/0/column", serde_json::json!(80)),
    ] {
        let mut invalid = serde_json::to_value(&hint).expect("hint wire data");
        *invalid
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("malformed hint pointer {pointer}")) = value;
        let invalid = serde_json::from_value(invalid).expect("malformed hint");
        let prepared = render_layout_with_prefix(
            source,
            80,
            MathPresentation::Native,
            Completion::Streaming,
            Some(&invalid),
        )
        .expect("canonical fallback");
        assert!(!prepared.reused_prefix, "{pointer}");
        let canonical = render_layout(source, 80, MathPresentation::Native, Completion::Streaming)
            .expect("canonical render");
        assert_eq!(prepared.layout, canonical, "{pointer}");
    }
}

/// MD-2/MD-4: a CJK grapheme ending exactly at the measured edge remains a reusable hint.
#[test]
fn frozen_prefix_accepts_an_exact_fitting_cjk_fragment() {
    let source = "# 中文\n\n";
    let width = 4;
    let first = render_layout_with_prefix(
        source,
        width,
        MathPresentation::Native,
        Completion::Streaming,
        None,
    )
    .expect("initial render");
    let checkpoint = first.checkpoint.expect("heading checkpoint");
    let layout = first
        .layout
        .prefix(checkpoint.rows(), checkpoint.visible_text_bytes())
        .expect("prefix rows");
    let hint = PrefixHint::new(checkpoint, layout).expect("bounded prefix");
    let reused = render_layout_with_prefix(
        source,
        width,
        MathPresentation::Native,
        Completion::Streaming,
        Some(&hint),
    )
    .expect("reused render");
    assert!(reused.reused_prefix);
}
