//! Markdown has content roles of its own; selecting them must not recolor workspace chrome.
use super::{Modifier, Palette, Role, Style};

/// Color choice for assistant Markdown, independent of the workspace and script footer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkdownTheme {
    /// Derive Markdown styles from the surrounding palette.
    #[default]
    Inherited,
    /// Use the designed palette's blue, green, cyan and yellow slots.
    Pastel,
}

pub(crate) struct MarkdownStyles {
    pub(crate) headings: [Style; 3],
    pub(crate) inline_code: Style,
    pub(crate) code: Style,
    pub(crate) link: Style,
    pub(crate) quote: Style,
    pub(crate) marker: Style,
    pub(crate) task_marker: Style,
    pub(crate) guide: Style,
    pub(crate) rule: Style,
    pub(crate) selection: Style,
}

impl Palette {
    /// Select assistant Markdown colors without changing any workspace role.
    #[must_use]
    pub const fn with_markdown_theme(mut self, theme: MarkdownTheme) -> Self {
        self.markdown = theme;
        self
    }

    pub(crate) fn markdown_styles(&self) -> MarkdownStyles {
        match self.markdown {
            MarkdownTheme::Inherited => MarkdownStyles {
                headings: [self
                    .style(Role::SectionHeading)
                    .add_modifier(Modifier::BOLD); 3],
                inline_code: self.style(Role::Accent),
                code: self.style(Role::Body),
                link: self.style(Role::Accent).add_modifier(Modifier::UNDERLINED),
                quote: self.style(Role::Muted),
                marker: self.style(Role::Muted),
                task_marker: self.style(Role::Accent),
                guide: self.style(Role::Muted),
                rule: self.style(Role::Border),
                selection: self.style(Role::Selection),
            },
            MarkdownTheme::Pastel => {
                // MD-5: the designed theme names slots rather than deriving from workspace roles,
                // because document structure is not workspace attention. Unbuilt: these are the
                // designed slots rather than the palette's own, so a swapped palette reaches the
                // inherited theme above and never this one.
                let slots = super::Slots::designed();
                MarkdownStyles {
                    // Blue, green, cyan lead the hierarchy; yellow marks what you would type.
                    headings: [slots.blue, slots.green, slots.cyan]
                        .map(|colour| Style::new().fg(colour).add_modifier(Modifier::BOLD)),
                    inline_code: Style::new().fg(slots.yellow).bg(slots.ground),
                    code: self.style(Role::Body),
                    link: Style::new()
                        .fg(slots.blue)
                        .add_modifier(Modifier::UNDERLINED),
                    quote: Style::new().fg(slots.muted).add_modifier(Modifier::ITALIC),
                    marker: Style::new().fg(slots.muted),
                    task_marker: Style::new().fg(slots.cyan),
                    guide: Style::new().fg(slots.muted),
                    rule: Style::new().fg(slots.line),
                    selection: Style::new().bg(slots.ground),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MD-5: a content color choice cannot change focus, selection, approval or other chrome.
    #[test]
    fn markdown_pastel_leaves_all_workspace_roles_unchanged() {
        let base = Palette::pastel().with_markdown_theme(MarkdownTheme::Inherited);
        let colored = base.with_markdown_theme(MarkdownTheme::Pastel);
        for role in Role::ALL {
            assert_eq!(base.style(role), colored.style(role), "{role:?}");
        }
        assert_ne!(
            base, colored,
            "the palette identity must include Markdown colors to request a repaint"
        );
        let styles = colored.markdown_styles();
        assert_eq!(
            styles.headings[0].fg,
            Some(super::super::Slots::designed().blue)
        );
        assert_eq!(
            styles.headings[1].fg,
            Some(super::super::Slots::designed().green)
        );
        assert_eq!(
            styles.headings[2].fg,
            Some(super::super::Slots::designed().cyan)
        );
        assert_eq!(styles.link.fg, Some(super::super::Slots::designed().blue));
        assert_eq!(
            styles.inline_code.fg,
            Some(super::super::Slots::designed().yellow)
        );
        let designed = Palette::pastel().markdown_styles();
        assert_eq!(
            designed.headings.map(|style| style.fg),
            styles.headings.map(|style| style.fg),
            "the designed palette and the Markdown theme are one set of tokens"
        );
        assert_eq!(designed.inline_code.fg, styles.inline_code.fg);
    }
}
