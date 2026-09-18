//! Code roles are content meaning, independent of workspace attention and error roles.
use ratatui::style::{Modifier, Style};
use serde::{Deserialize, Serialize};

use super::{MarkdownTheme, Palette, Role, Slots};

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
        // MD-5: the designed theme names slots rather than deriving from workspace roles, because
        // code meaning is not workspace attention — a keyword is not "where you are". It still
        // reaches no further than the slots, so a theme reaches it. Unbuilt: these slots are the
        // designed ones rather than the palette's own, so a swapped palette leaves code as it was.
        let slots = Slots::designed();
        match role {
            CodeRole::Text => self.style(Role::Body),
            CodeRole::Keyword => Style::new().fg(slots.blue),
            CodeRole::Type | CodeRole::Property => Style::new().fg(slots.cyan),
            CodeRole::Function => Style::new().fg(slots.yellow),
            CodeRole::String => Style::new().fg(slots.green),
            CodeRole::Constant => Style::new().fg(slots.orange),
            CodeRole::Comment => Style::new().fg(slots.muted).add_modifier(Modifier::ITALIC),
        }
    }
}
