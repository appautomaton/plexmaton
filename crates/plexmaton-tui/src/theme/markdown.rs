//! Markdown has content roles of its own; selecting them must not recolor workspace chrome.
use super::{Modifier, Palette, Role, Style};

/// Color choice for assistant Markdown, independent of the workspace and script footer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkdownTheme {
    /// Derive Markdown styles from the surrounding palette, including monochrome.
    #[default]
    Inherited,
    /// Use the existing pastel palette's blue, green, lavender, teal and warm yellow accents.
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
            },
            MarkdownTheme::Pastel => {
                let colors = Self::pastel();
                MarkdownStyles {
                    headings: [Role::Ambient, Role::NewInformation, Role::Accent]
                        .map(|role| colors.style(role).add_modifier(Modifier::BOLD)),
                    inline_code: Style {
                        fg: colors.style(Role::ActionRequired).fg,
                        ..Style::default()
                    },
                    code: self.style(Role::Body),
                    link: colors
                        .style(Role::BorderFocused)
                        .add_modifier(Modifier::UNDERLINED),
                    quote: colors.style(Role::Muted).add_modifier(Modifier::ITALIC),
                    marker: colors.style(Role::Ambient),
                    task_marker: colors.style(Role::Ambient),
                    guide: colors.style(Role::Muted),
                    rule: colors.style(Role::Border),
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
        let base = Palette::ansi();
        let colored = base.with_markdown_theme(MarkdownTheme::Pastel);
        for role in Role::ALL {
            assert_eq!(base.style(role), colored.style(role), "{role:?}");
        }
        assert_ne!(
            base, colored,
            "the palette identity must include Markdown colors to request a repaint"
        );
        let styles = colored.markdown_styles();
        let pastel = Palette::pastel();
        assert_eq!(styles.headings[0].fg, pastel.style(Role::Ambient).fg);
        assert_eq!(styles.headings[1].fg, pastel.style(Role::NewInformation).fg);
        assert_eq!(styles.headings[2].fg, pastel.style(Role::Accent).fg);
        assert_eq!(styles.link.fg, pastel.style(Role::BorderFocused).fg);
        assert_eq!(styles.inline_code.fg, pastel.style(Role::ActionRequired).fg);
    }
}
