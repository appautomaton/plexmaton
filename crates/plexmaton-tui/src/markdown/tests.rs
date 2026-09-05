use super::*;

fn text(lines: &[Line<'_>]) -> String {
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
        let before = render_layout(source, &base, width).expect("inherited");
        let after = render_layout(source, &proposed, width).expect("pastel");
        assert_eq!(before.text, after.text, "copy source at {width}");
        assert_eq!(before.rows.len(), after.rows.len());
        for (before, after) in before.rows.iter().zip(&after.rows) {
            assert_eq!(before.len(), after.len());
            for (before, after) in before.iter().zip(after) {
                assert_eq!(
                    (before.column, &before.text),
                    (after.column, &after.text),
                    "copy fragments at {width}"
                );
            }
        }
        assert_eq!(
            text(&before.lines),
            text(&after.lines),
            "wrapping at {width}"
        );
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
    let pastel = Palette::pastel();
    assert_eq!(style("Blue heading").fg, pastel.style(Role::Ambient).fg);
    assert_eq!(
        style("Green heading").fg,
        pastel.style(Role::NewInformation).fg
    );
    assert_eq!(style("Lavender heading").fg, pastel.style(Role::Accent).fg);
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
