//! The workspace's own command list.
//!
//! It belongs to the workspace rather than to a conversation, which is what separates it from an
//! approval: an approval is a question one agent is waiting on and renders inside that agent's box,
//! while this is opened by the user from anywhere and floats over everything. Its filter is a
//! [`TextInput`] like any other, so the editing grammar is the composer's without being the
//! composer's code.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{Caret, TextInput};
use crate::surface::SurfaceId;

/// One thing the workspace can be asked to do.
///
/// The built-in commands share typed dispatch; aliases only affect discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    /// Show the resolved provider, model and reasoning effort.
    Config,
    /// Find and resume an existing conversation without dispatching a request.
    Resume,
    /// Start an empty conversation; storage is created on its first submitted message.
    New,
    /// Review grants, enable Session file changes, or revoke a permission.
    Permissions,
}

impl Command {
    pub(crate) fn from_slash(text: &str) -> Option<Self> {
        let name = text.trim().strip_prefix('/')?;
        Self::ALL.into_iter().find(|command| {
            command.name().trim_start_matches('/') == name || command.aliases().contains(&name)
        })
    }
    /// Every command, in the order the list shows them.
    pub const ALL: [Self; 4] = [Self::Config, Self::Resume, Self::New, Self::Permissions];

    /// The name the list shows and the user types.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Config => "/config",
            Self::Resume => "/resume",
            Self::New => "/new",
            Self::Permissions => "/permissions",
        }
    }

    /// What running it does, shown beside the name.
    #[must_use]
    pub const fn summary(self) -> &'static str {
        match self {
            Self::Config => "Provider, model, and reasoning effort",
            Self::Resume => "Find and resume a saved conversation",
            Self::New => "Start a new conversation",
            Self::Permissions => "Review and change Session or Project permissions",
        }
    }

    /// Other words that find this command.
    ///
    /// Matching keys, never rows: one command is one line in the list whatever the user typed to
    /// reach it, so the vocabulary still has one name for the thing.
    #[must_use]
    pub const fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Config => &["settings"],
            Self::Resume => &["continue", "sessions", "session"],
            Self::New | Self::Permissions => &[],
        }
    }

    fn matches(self, needle: &str) -> bool {
        let needle = needle.trim().trim_start_matches('/').to_lowercase();
        if needle.is_empty() {
            return true;
        }
        self.name().trim_start_matches('/').contains(&needle)
            || self
                .aliases()
                .iter()
                .any(|alias| alias.contains(needle.as_str()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PalettePage {
    Commands,
    Conversations(super::session_picker::ConversationPicker),
    Permissions(Box<super::permissions::PermissionPanel>),
}

/// The command list while it is open.
///
/// Absent when closed rather than carrying an `open` flag, so "is it open" and "what is it showing"
/// cannot disagree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandPalette {
    pub(crate) page: PalettePage,
    filter: TextInput,
    /// Index into the *matching* commands, clamped every time the filter changes.
    chosen: usize,
    /// Where focus goes when the list closes, captured when it opened.
    return_focus: SurfaceId,
}

impl CommandPalette {
    /// An empty filter with the first match chosen, remembering where focus came from.
    #[must_use]
    pub fn opened_from(return_focus: SurfaceId) -> Self {
        Self {
            page: PalettePage::Commands,
            filter: TextInput::new(),
            chosen: 0,
            return_focus,
        }
    }

    /// Where focus goes when the list closes.
    #[must_use]
    pub const fn return_focus(&self) -> SurfaceId {
        self.return_focus
    }

    /// The filter the user is typing into.
    #[must_use]
    pub const fn filter(&self) -> &TextInput {
        &self.filter
    }

    /// One horizontal window and its caret, inside the shared surface padding (COM-1).
    /// Reserve a cell for the caret so it never lands on the border or wraps into the results.
    pub(crate) fn filter_view(&self, width: u16) -> (String, Caret) {
        let (_, visible, caret) = self.filter_window(width);
        (visible, caret)
    }

    /// Resolves a click against the same horizontal window used for painting (COM-1).
    pub(crate) fn click_filter(&mut self, width: u16, column: u16) -> bool {
        self.filter.place_caret(self.filter_offset(width, column))
    }

    pub(crate) fn drag_filter(&mut self, width: u16, column: u16) {
        self.filter
            .drag_to_offset(self.filter_offset(width, column));
    }

    pub(crate) fn filter_range(&self, width: u16) -> std::ops::Range<usize> {
        let (start, text, _) = self.filter_window(width);
        start..start + text.len()
    }

    fn filter_offset(&self, width: u16, column: u16) -> usize {
        let (mut offset, visible, _) = self.filter_window(width);
        let mut used = 0;
        for cluster in visible.graphemes(true) {
            used += UnicodeWidthStr::width(cluster);
            if used > usize::from(column) {
                break;
            }
            offset += cluster.len();
        }
        offset
    }

    fn filter_window(&self, width: u16) -> (usize, String, Caret) {
        let budget = usize::from(width.saturating_sub(1));
        let text = self.filter.text();
        let cursor = self.filter.cursor();
        let mut start = cursor;
        let mut before = 0;
        for (offset, cluster) in text[..cursor].grapheme_indices(true).rev() {
            let cells = UnicodeWidthStr::width(cluster);
            if before + cells > budget {
                break;
            }
            start = offset;
            before += cells;
        }
        let mut visible = String::new();
        let mut used = 0;
        for cluster in text[start..].graphemes(true) {
            let cells = UnicodeWidthStr::width(cluster);
            if used + cells > budget {
                break;
            }
            visible.push_str(cluster);
            used += cells;
        }
        (
            start,
            visible,
            Caret {
                row: 0,
                column: u16::try_from(before).unwrap_or(0),
            },
        )
    }

    pub(crate) fn conversations(&self) -> Option<&super::session_picker::ConversationPicker> {
        match &self.page {
            PalettePage::Conversations(page) => Some(page),
            _ => None,
        }
    }
    pub(crate) fn conversations_mut(
        &mut self,
    ) -> Option<&mut super::session_picker::ConversationPicker> {
        match &mut self.page {
            PalettePage::Conversations(page) => Some(page),
            _ => None,
        }
    }
    pub(crate) fn permissions(&self) -> Option<&super::permissions::PermissionPanel> {
        match &self.page {
            PalettePage::Permissions(page) => Some(page),
            _ => None,
        }
    }
    pub(crate) fn permissions_mut(&mut self) -> Option<&mut super::permissions::PermissionPanel> {
        match &mut self.page {
            PalettePage::Permissions(page) => Some(page),
            _ => None,
        }
    }

    /// The filter, for editing.
    pub const fn filter_mut(&mut self) -> &mut TextInput {
        &mut self.filter
    }

    /// Commands the current filter admits, in list order.
    #[must_use]
    pub fn matches(&self) -> Vec<Command> {
        if !matches!(self.page, PalettePage::Commands) {
            return Vec::new();
        }
        let needle = self.filter.text();
        Command::ALL
            .into_iter()
            .filter(|command| command.matches(needle))
            .collect()
    }

    /// Which match is chosen, or nothing when the filter admits none.
    #[must_use]
    pub fn chosen(&self) -> Option<Command> {
        self.matches().get(self.chosen).copied()
    }

    /// Index of the chosen row among the matches, for rendering the marker.
    #[must_use]
    pub const fn chosen_index(&self) -> usize {
        self.chosen
    }

    /// Moves the choice by one, stopping at the ends rather than wrapping.
    ///
    /// Stopping is what the approval card does with the same keys; wrapping in a filtered list also
    /// means a held arrow silently returns to where it started.
    pub fn step(&mut self, forward: bool) -> bool {
        if let Some(panel) = self.permissions_mut() {
            return panel.step(forward);
        }
        if self
            .conversations()
            .is_some_and(|s| s.status == super::ConversationPickerStatus::Opening)
        {
            return false;
        }
        let last = self.match_count().saturating_sub(1);
        let next = if forward {
            self.chosen.saturating_add(1).min(last)
        } else {
            self.chosen.saturating_sub(1)
        };
        if next == self.chosen {
            return false;
        }
        self.chosen = next;
        true
    }

    /// Re-clamps the choice after the filter changed, so it always names a visible row.
    pub fn reclamp(&mut self) {
        self.chosen = self.chosen.min(self.match_count().saturating_sub(1));
    }

    pub(crate) fn match_count(&self) -> usize {
        self.conversations().map_or_else(
            || self.matches().len(),
            |sessions| sessions.matches(self.filter.text()).len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, CommandPalette};
    use crate::surface::SurfaceId;

    fn filtered(text: &str) -> CommandPalette {
        let mut palette = CommandPalette::opened_from(SurfaceId::Composer);
        for character in text.chars() {
            palette.filter_mut().insert(character);
        }
        palette.reclamp();
        palette
    }

    /// One command is one row whatever the user typed to reach it.
    #[test]
    fn an_alias_finds_its_command_without_adding_a_second_row() {
        for typed in [
            "conf",
            "/conf",
            "config",
            "/config",
            "settings",
            "/settings",
            "SETTING",
        ] {
            let palette = filtered(typed);
            assert_eq!(palette.matches(), vec![Command::Config], "typed {typed:?}");
        }
    }

    /// A filter that admits nothing chooses nothing, rather than a stale row.
    #[test]
    fn a_filter_matching_nothing_leaves_no_choice() {
        let palette = filtered("nothing-matches-this");
        assert!(palette.matches().is_empty());
        assert_eq!(palette.chosen(), None);
    }

    /// The choice stops at the ends rather than wrapping.
    #[test]
    fn stepping_stops_at_the_ends_of_the_matches() {
        let mut palette = filtered("");
        assert!(!palette.step(false));
        for command in Command::ALL.into_iter().skip(1) {
            assert!(palette.step(true));
            assert_eq!(palette.chosen(), Some(command));
        }
        assert!(!palette.step(true));
    }
}
