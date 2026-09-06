use super::paint::*;
use crate::{Palette, Role};
use ratatui::style::{Color, Modifier, Style};

/// MD-5: semantic preparation preserves ordered color/modifier patches, even when a palette
/// removes a modifier or leaves a foreground unspecified. Resolving only the last role is wrong.
#[test]
fn semantic_paint_keeps_custom_role_patch_order_and_nested_markdown() {
    let custom = Palette::from_roles(|role| match role {
        Role::Body => Style::new().fg(Color::Green).add_modifier(Modifier::ITALIC),
        Role::SectionHeading => Style::new().fg(Color::Blue).add_modifier(Modifier::DIM),
        Role::Accent => Style::new()
            .bg(Color::Red)
            .remove_modifier(Modifier::ITALIC),
        _ => Style::new(),
    });
    let source = "# *[go `now`](https://example.invalid)*";
    let prepared = crate::markdown::render_layout(
        source,
        88,
        crate::math::MathPresentation::Native,
        crate::markdown::Completion::Final,
    )
    .expect("nested Markdown");
    for palette in [
        custom,
        Palette::ansi(),
        Palette::pastel(),
        Palette::monochrome(),
    ] {
        let lines = prepared.painted_lines(&palette);
        let styles = palette.markdown_styles();
        let expected = styles.headings[0]
            .add_modifier(Modifier::ITALIC)
            .patch(styles.link);
        let find = |text| {
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .find(|span| span.content == text)
                .expect("nested span")
                .style
        };
        assert_eq!(find("go "), expected);
        assert_eq!(find("now"), expected.patch(styles.inline_code));
        let colors = Colors::new(&palette);
        let retained = Line::from(vec![Span::styled(
            "source",
            Paint::from(Role::Body)
                .add_modifier(Modifier::BOLD)
                .patch(Role::Accent.into()),
        )]);
        assert_eq!(
            retained.paint_ref(&colors).spans[0].style,
            palette
                .style(Role::Body)
                .add_modifier(Modifier::BOLD)
                .patch(palette.style(Role::Accent))
        );
    }
}

/// MD-4: composed style allocations count toward the cache's byte bound, not just text/spans.
#[test]
fn retained_style_accounting_includes_composed_patch_capacity() {
    let role = Paint::from(Role::Body);
    assert_eq!(
        role.allocation_bytes(),
        0,
        "simple roles allocate no patch chain"
    );
    let nested = role
        .add_modifier(Modifier::BOLD)
        .patch(MarkdownRole::Link.into());
    assert!(nested.allocation_bytes() > 0);
    let mut prepared = crate::markdown::render_layout(
        "**[go](https://example.invalid)**",
        60,
        crate::math::MathPresentation::Native,
        crate::markdown::Completion::Final,
    )
    .expect("composed style");
    let before = prepared.allocation_bytes();
    let mut style_bytes = 0;
    for line in &mut prepared.lines {
        style_bytes += line.style.allocation_bytes();
        line.style = Paint::default();
        for span in &mut line.spans {
            style_bytes += span.style.allocation_bytes();
            span.style = Paint::default();
        }
    }
    assert!(style_bytes > 0, "fixture must retain composed styles");
    assert_eq!(before - prepared.allocation_bytes(), style_bytes);
}
