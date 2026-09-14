use super::*;
use crate::{
    Palette,
    markdown::{Completion, render_layout},
    math::MathPresentation,
};

fn role_at(tokens: &[Token], source: &str, needle: &str) -> CodeRole {
    let offset = source.find(needle).expect("fixture token");
    tokens
        .iter()
        .find(|token| token.bytes.contains(&offset))
        .expect("covered byte")
        .role
}

/// MD-6: each admitted grammar parses real syntax, including aliases and multiline context.
#[test]
fn syntax_grammars_color_language_constructs_and_preserve_every_byte() {
    for (language, source, samples) in [
        (
            "RS",
            "fn greet() -> u32 { /* let\nfn */ let s = r#\"hello\n世界\"#; 42 }",
            vec![
                ("fn greet", CodeRole::Keyword),
                ("greet", CodeRole::Function),
                ("u32", CodeRole::Type),
                ("/*", CodeRole::Comment),
                ("fn */", CodeRole::Comment),
                ("世界", CodeRole::String),
                ("42", CodeRole::Constant),
            ],
        ),
        (
            "py",
            "def greet():\n    s = \"\"\"hello\ndef 世界\"\"\"\n    return 42\n",
            vec![
                ("def greet", CodeRole::Keyword),
                ("greet", CodeRole::Function),
                ("def 世界", CodeRole::String),
                ("42", CodeRole::Constant),
            ],
        ),
        (
            "json",
            "{\"answer\": 42, \"ready\": true, \"text\": \"世界\"}",
            vec![
                ("answer", CodeRole::Property),
                ("42", CodeRole::Constant),
                ("true", CodeRole::Constant),
                ("世界", CodeRole::String),
            ],
        ),
        (
            "js",
            "function greet() { return \"hello\"; }",
            vec![
                ("function", CodeRole::Keyword),
                ("greet", CodeRole::Function),
                ("hello", CodeRole::String),
            ],
        ),
        (
            "ts",
            "function greet(x: number): string { return \"hello\"; }",
            vec![
                ("function", CodeRole::Keyword),
                ("greet", CodeRole::Function),
                ("number", CodeRole::Type),
            ],
        ),
        (
            "tsx",
            "const title = <Panel title=\"hello\" />;",
            vec![("const", CodeRole::Keyword), ("hello", CodeRole::String)],
        ),
        (
            "shell",
            "# keep this\nif true; then echo \"hello\"; fi",
            vec![
                ("# keep", CodeRole::Comment),
                ("if", CodeRole::Keyword),
                ("hello", CodeRole::String),
            ],
        ),
    ] {
        let tokens = CodeHighlighter::default()
            .highlight(language, source)
            .expect(language);
        assert_eq!(
            tokens
                .iter()
                .map(|token| &source[token.bytes.clone()])
                .collect::<String>(),
            source
        );
        for (needle, role) in samples {
            assert_eq!(
                role_at(&tokens, source, needle),
                role,
                "{language}: {needle}"
            );
        }
    }
}

/// MD-6: a grammar is compiled once per process, not once per render.
///
/// Compiling a highlight query costs two orders of magnitude more than highlighting an ordinary
/// block with it, so an owner narrower than the process turns every code fence into that cost.
#[test]
fn syntax_grammars_compile_once_and_are_shared_by_every_renderer() {
    let compiled = |info: &str| {
        languages::Language::from_info(info)
            .expect(info)
            .configuration()
            .expect("bundled grammar compiles")
    };
    for (first, second) in [("rust", "rs"), ("python", "py"), ("ts", "typescript")] {
        assert!(
            std::ptr::eq(compiled(first), compiled(second)),
            "{first}/{second} recompiled instead of sharing one configuration"
        );
    }
    let bundled = ["rs", "py", "json", "js", "ts", "tsx", "sh"].map(|info| {
        languages::Language::from_info(info)
            .expect(info)
            .configuration()
            .expect("bundled grammar compiles")
    });
    for (position, one) in bundled.iter().enumerate() {
        for other in &bundled[position + 1..] {
            assert!(
                !std::ptr::eq(*one, *other),
                "two languages resolved to one compiled grammar"
            );
        }
    }
    // Two renderers are two renders, and neither owns the grammar it reads.
    let before = compiled("rust");
    CodeHighlighter::default()
        .highlight("rust", "fn main() {}")
        .expect("first render");
    CodeHighlighter::default()
        .highlight("rust", "fn main() {}")
        .expect("second render");
    assert!(std::ptr::eq(before, compiled("rust")));
}

/// MD-6/MD-3: language text cannot load a grammar, and budget exhaustion preserves whole blocks.
#[test]
fn syntax_unknown_and_budget_fallbacks_keep_complete_literal_code() {
    for (language, reason) in [
        ("", PlainCode::Unspecified),
        ("text", PlainCode::Unspecified),
        ("../../rust", PlainCode::UnknownLanguage),
        ("language-that-does-not-exist", PlainCode::UnknownLanguage),
    ] {
        assert_eq!(
            CodeHighlighter::default()
                .highlight(language, "fn main() {}")
                .err(),
            Some(reason)
        );
    }
    let mut engine = CodeHighlighter::default();
    let source = " ".repeat(MAX_CODE_BYTES);
    assert!(engine.highlight("rust", &source).is_ok());
    assert_eq!(
        engine
            .highlight("rust", &"x".repeat(MAX_CODE_BYTES + 1))
            .err(),
        Some(PlainCode::Limit)
    );
    let source = format!("```rust\n{}\n```", "x".repeat(MAX_CODE_BYTES + 1));
    let layout = render_layout(&source, 60, MathPresentation::Native, Completion::Final)
        .expect("plain code");
    assert!(layout.text.contains(&"x".repeat(MAX_CODE_BYTES + 1)));
    assert!(
        layout
            .lines
            .iter()
            .any(|line| line.to_string().contains("syntax limit; plain text"))
    );
}

/// MD-1/MD-2/MD-5/MD-6: styling and selection preserve wrapped Unicode and literal code copy.
#[test]
fn syntax_paint_selection_and_monochrome_share_exact_code_geometry() {
    let code = "fn greet() {\n\tlet message = \"世界 e\u{301} **literal**\"; // quiet\n}\n";
    let source = format!("```rust\n{code}```");
    for width in [8, 60, 88, 120] {
        let layout = render_layout(&source, width, MathPresentation::Native, Completion::Final)
            .expect("layout");
        assert_eq!(
            layout.text,
            code.replace('\t', "    ").trim_end_matches('\n')
        );
        assert!(layout.text_fragments_within_width(width));
        let pastel = layout.painted_lines(&Palette::pastel());
        let mono = layout.painted_lines(&Palette::monochrome());
        let selected = layout.painted_entry(
            &Palette::pastel(),
            crate::state::EntryAppearance {
                selected: true,
                ..Default::default()
            },
        );
        assert_eq!(
            pastel.iter().map(ToString::to_string).collect::<Vec<_>>(),
            mono.iter().map(ToString::to_string).collect::<Vec<_>>()
        );
        assert_eq!(
            pastel.iter().map(ToString::to_string).collect::<Vec<_>>(),
            selected.iter().map(ToString::to_string).collect::<Vec<_>>()
        );
        let token = |lines: &[ratatui::text::Line<'_>]| {
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .find(|span| span.content == "fn")
                .expect("keyword")
                .style
        };
        assert_eq!(token(&pastel).fg, Some(crate::theme::tokens::SKY));
        assert_eq!(token(&selected).fg, token(&pastel).fg);
        assert_eq!(token(&selected).bg, Some(crate::theme::tokens::BAR));
        assert!(mono.iter().all(|line| {
            line.style.fg.is_none()
                && line.style.bg.is_none()
                && line
                    .spans
                    .iter()
                    .all(|span| span.style.fg.is_none() && span.style.bg.is_none())
        }));
    }
}

/// MD-3/MD-6: partial source, including unfinished comments and strings, never loses visible bytes.
#[test]
fn syntax_streaming_open_fences_remain_literal_and_finish_canonically() {
    let source = "```rust\nfn main() {\n let s = \"世界\"; /* x\n y */\n}\n```";
    for (end, _) in source
        .char_indices()
        .chain(std::iter::once((source.len(), '\0')))
    {
        let prefix = &source[..end];
        let streaming = render_layout(prefix, 60, MathPresentation::Native, Completion::Streaming)
            .expect("streaming");
        let final_layout =
            render_layout(prefix, 60, MathPresentation::Native, Completion::Final).expect("final");
        assert_eq!(streaming, final_layout, "prefix {prefix:?}");
        assert!(streaming.text_fragments_within_width(60));
    }
}

/// MD-6: a malformed or excessive event stream discards partial styling, never source bytes.
#[test]
fn syntax_event_limits_and_invalid_ranges_refuse_partial_highlights() {
    let excessive = std::iter::repeat_with(|| Ok(HighlightEvent::Source { start: 0, end: 0 }))
        .take(MAX_CODE_EVENTS + 1);
    assert_eq!(collect_tokens("", excessive).err(), Some(PlainCode::Limit));
    assert_eq!(
        collect_tokens(
            "世界",
            [Ok(HighlightEvent::Source { start: 0, end: 1 })].into_iter()
        )
        .err(),
        Some(PlainCode::Unavailable)
    );
    assert_eq!(
        collect_tokens(
            "abc",
            [Ok(HighlightEvent::Source { start: 1, end: 3 })].into_iter()
        )
        .err(),
        Some(PlainCode::Unavailable)
    );
}

/// MD-4/MD-6: a physically closed code fence is reused; open fences and replacements are not frozen.
#[test]
fn syntax_streaming_reuses_closed_fences_without_rehighlighting_the_prefix() {
    use crate::markdown::{PrefixHint, render_layout_with_prefix};
    for width in [60, 88, 120] {
        for fence in ["```", "~~~~"] {
            let initial = format!("{fence}rust\nfn greet() {{}}\n{fence}\n\nTail");
            let first = render_layout_with_prefix(
                &initial,
                width,
                MathPresentation::Native,
                Completion::Streaming,
                None,
            )
            .expect("first");
            assert_eq!(first.code_preparations, 1);
            let checkpoint = first.checkpoint.clone().expect("closed fence checkpoint");
            assert!(
                checkpoint
                    .source_prefix()
                    .ends_with(&format!("{fence}\n\n"))
            );
            let prefix = first
                .layout
                .prefix(checkpoint.rows(), checkpoint.visible_text_bytes())
                .expect("prefix rows");
            let hint = PrefixHint::new(checkpoint, prefix).expect("bounded hint");
            for suffix in [
                " continues",
                "\n\n```python\ndef more():\n    return 42\n```\n\nEnd",
                "\n\n```json\n{\"ready\": true}",
            ] {
                let source = format!("{initial}{suffix}");
                let next = render_layout_with_prefix(
                    &source,
                    width,
                    MathPresentation::Native,
                    Completion::Streaming,
                    Some(&hint),
                )
                .expect("suffix");
                assert!(next.reused_prefix);
                assert_eq!(
                    next.code_preparations,
                    usize::from(suffix.contains("```")),
                    "only the new fence parses"
                );
                let canonical = render_layout(
                    &source,
                    width,
                    MathPresentation::Native,
                    Completion::Streaming,
                )
                .expect("canonical");
                assert_eq!(next.layout, canonical, "{fence} at {width}: {suffix}");
            }
            let changed = initial.replace("greet", "changed");
            assert!(
                !render_layout_with_prefix(
                    &changed,
                    width,
                    MathPresentation::Native,
                    Completion::Streaming,
                    Some(&hint)
                )
                .expect("replacement")
                .reused_prefix
            );
        }
    }
    for source in [
        "```rust\nfn greet() {}\n\nTail",
        "````rust\nfn greet() {}\n```\n\nTail",
        "    code\n\nTail",
        "```rust\n\n",
        "~~~rust\nfn greet() {}\n```\n\nTail",
    ] {
        let result = render_layout_with_prefix(
            source,
            60,
            MathPresentation::Native,
            Completion::Streaming,
            None,
        )
        .expect("open fence");
        assert!(result.checkpoint.is_none(), "unsafe cut: {source:?}");
    }
}
