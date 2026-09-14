//! Code roles are content meaning, independent of workspace attention and error roles.
use ratatui::style::{Modifier, Style};
use serde::{Deserialize, Serialize};

use super::{MarkdownTheme, Palette, Role, tokens};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum CodeRole {
    Text,
    Keyword,
    Type,
    Function,
    String,
    Constant,
    Comment,
    Property,
}

impl Palette {
    pub(crate) fn code_style(&self, role: CodeRole) -> Style {
        if self.markdown == MarkdownTheme::Inherited {
            return match role {
                CodeRole::Keyword => self.style(Role::Body).add_modifier(Modifier::BOLD),
                CodeRole::Comment => self.style(Role::Muted).add_modifier(Modifier::ITALIC),
                _ => self.style(Role::Body),
            };
        }
        match role {
            CodeRole::Text => self.style(Role::Body),
            CodeRole::Keyword => Style::new().fg(tokens::SKY),
            CodeRole::Type | CodeRole::Property => Style::new().fg(tokens::TEAL),
            CodeRole::Function => Style::new().fg(tokens::GOLD),
            CodeRole::String => Style::new().fg(tokens::MINT),
            CodeRole::Constant => Style::new().fg(tokens::ORANGE),
            CodeRole::Comment => Style::new()
                .fg(tokens::STEEL)
                .add_modifier(Modifier::ITALIC),
        }
    }
}
