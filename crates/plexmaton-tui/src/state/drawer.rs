//! The workspace's own input.
//!
//! Pulled from the top edge by `Ctrl-P`, it addresses the workspace rather than a conversation.
//! That is what separates it from an approval, a question one agent is waiting on inside that
//! agent's box, and from the composer menu, which completes what a conversation's input is typing.
//! It holds pages, never commands: a `/name` is typed where its conversation is. Its filter is a
//! [`TextInput`] like any other, so the editing grammar is the composer's without being the
//! composer's code.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{Caret, TextInput, configuration::ConfigurationSummary, permissions::PermissionPanel};
use crate::surface::SurfaceId;

/// One view inside the Drawer (ui-ux §product vocabulary).
///
/// The list shows these rows; choosing one hands the page to the composition root, which owns
/// what opening it costs: a permission store read, or the resolved model. Conversations are
/// not here: starting or resuming one is a Command typed where the user types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Page {
    /// The provider, model and reasoning effort this process resolved.
    Configuration,
    /// Session and Project grants: review, enable, revoke.
    Permissions,
}

impl Page {
    /// Every page, in the order the list shows them.
    pub const ALL: [Self; 2] = [Self::Configuration, Self::Permissions];

    /// The name the list shows and the title carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Configuration => "Configuration",
            Self::Permissions => "Permissions",
        }
    }

    /// What the page holds, shown beside the name.
    #[must_use]
    pub const fn summary(self) -> &'static str {
        match self {
            Self::Configuration => "Provider, model, and reasoning effort",
            Self::Permissions => "Grants and trust for this Project",
        }
    }

    /// Plain substring on the name. A leading `/` is a character like any other here: commands
    /// are typed in a conversation, and the Drawer must not look as if it took them.
    fn matches(self, needle: &str) -> bool {
        let needle = needle.trim().to_lowercase();
        needle.is_empty() || self.name().to_lowercase().contains(&needle)
    }
}

/// What the Drawer is showing: its list of pages, or one page opened in place.
///
/// A page carries its own state, so `Escape` back to the list finds the list's filter and choice
/// exactly as they were left (SURF-5), and a page reopened starts fresh.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Shown {
    Pages,
    Configuration(ConfigurationSummary),
    Permissions(Box<PermissionPanel>),
}

/// The Drawer while it is open.
///
/// Absent when closed rather than carrying an `open` flag, so "is it open" and "what is it showing"
/// cannot disagree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Drawer {
    pub(crate) shown: Shown,
    /// The page list's query.
    filter: TextInput,
    /// Index into the *matching* pages, clamped every time the filter changes.
    chosen: usize,
    /// Where focus goes when the Drawer closes, captured when it opened.
    return_focus: SurfaceId,
}

impl Drawer {
    /// The page list with an empty filter and the first page chosen, remembering where focus
    /// came from.
    #[must_use]
    pub fn opened_from(return_focus: SurfaceId) -> Self {
        Self {
            shown: Shown::Pages,
            filter: TextInput::new(),
            chosen: 0,
            return_focus,
        }
    }

    /// Where focus goes when the Drawer closes.
    #[must_use]
    pub const fn return_focus(&self) -> SurfaceId {
        self.return_focus
    }

    /// Which page is open, or nothing while the list shows.
    #[must_use]
    pub const fn page(&self) -> Option<Page> {
        match self.shown {
            Shown::Pages => None,
            Shown::Configuration(_) => Some(Page::Configuration),
            Shown::Permissions(_) => Some(Page::Permissions),
        }
    }

    /// The query the user is typing into: the list's.
    #[must_use]
    pub const fn filter(&self) -> &TextInput {
        &self.filter
    }

    /// Whether what is shown is typed into; the pages are navigated (SURF-3).
    #[must_use]
    pub const fn takes_text(&self) -> bool {
        matches!(self.shown, Shown::Pages)
    }

    /// The query, for editing. Pages hand back nothing, so no keystroke can edit a filter the
    /// screen does not show.
    pub const fn filter_mut(&mut self) -> Option<&mut TextInput> {
        match &mut self.shown {
            Shown::Pages => Some(&mut self.filter),
            Shown::Configuration(_) | Shown::Permissions(_) => None,
        }
    }

    /// One horizontal window and its caret, inside the shared surface padding (COM-1).
    /// Reserve a cell for the caret so it never lands on the border or wraps into the rows.
    pub(crate) fn filter_view(&self, width: u16) -> (String, Caret) {
        let (_, visible, caret) = self.filter_window(width);
        (visible, caret)
    }

    /// Resolves a click against the same horizontal window used for painting (COM-1).
    pub(crate) fn click_filter(&mut self, width: u16, column: u16) -> bool {
        let offset = self.filter_offset(width, column);
        self.filter_mut()
            .is_some_and(|filter| filter.place_caret(offset))
    }

    pub(crate) fn drag_filter(&mut self, width: u16, column: u16) {
        let offset = self.filter_offset(width, column);
        if let Some(filter) = self.filter_mut() {
            filter.drag_to_offset(offset);
        }
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
        let filter = self.filter();
        let text = filter.text();
        let cursor = filter.cursor();
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

    pub(crate) const fn configuration(&self) -> Option<&ConfigurationSummary> {
        match &self.shown {
            Shown::Configuration(summary) => Some(summary),
            _ => None,
        }
    }
    pub(crate) fn permissions(&self) -> Option<&PermissionPanel> {
        match &self.shown {
            Shown::Permissions(page) => Some(page),
            _ => None,
        }
    }
    pub(crate) fn permissions_mut(&mut self) -> Option<&mut PermissionPanel> {
        match &mut self.shown {
            Shown::Permissions(page) => Some(page),
            _ => None,
        }
    }

    /// Pages the list's filter admits, in list order. Empty while a page is open.
    #[must_use]
    pub fn pages(&self) -> Vec<Page> {
        if self.shown != Shown::Pages {
            return Vec::new();
        }
        let needle = self.filter.text();
        Page::ALL
            .into_iter()
            .filter(|page| page.matches(needle))
            .collect()
    }

    /// Which page is chosen in the list, or nothing when the filter admits none.
    #[must_use]
    pub fn chosen_page(&self) -> Option<Page> {
        self.pages().get(self.chosen).copied()
    }

    /// Index of the chosen row among what is listed, for rendering the marker.
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
        let last = self.match_count().saturating_sub(1);
        let current = self.chosen_index();
        let next = if forward {
            current.saturating_add(1).min(last)
        } else {
            current.saturating_sub(1)
        };
        if next == current {
            return false;
        }
        self.set_chosen(next);
        true
    }

    /// Re-clamps the choice after the filter changed, so it always names a visible row.
    pub fn reclamp(&mut self) {
        let last = self.match_count().saturating_sub(1);
        self.set_chosen(self.chosen_index().min(last));
    }

    const fn set_chosen(&mut self, index: usize) {
        self.chosen = index;
    }

    pub(crate) fn match_count(&self) -> usize {
        self.pages().len()
    }

    /// The rows of the list on screen, sliding so the chosen one stays inside (DRW-2).
    pub(crate) fn choice_window(&self, height: u16) -> std::ops::Range<usize> {
        let reserved = 4
            + 2 * self.choice_gap(height)
            + 2 * crate::surface::ContentInsets::for_surface(SurfaceId::Drawer, height).vertical;
        let count = usize::from(height.saturating_sub(reserved)).clamp(1, Page::ALL.len());
        let start = self.chosen_index().saturating_sub(count - 1);
        start..start + count
    }

    /// DRW-2: optional spacing yields before the selected row or the footer.
    pub(crate) fn choice_gap(&self, height: u16) -> u16 {
        u16::from(self.content_height(height) >= 5)
    }

    fn content_height(&self, height: u16) -> u16 {
        height.saturating_sub(
            2 + 2 * crate::surface::ContentInsets::for_surface(SurfaceId::Drawer, height).vertical,
        )
    }

    /// Rows the Drawer asks layout for, borders included.
    ///
    /// Filter, rows, footer, interior gaps, padding and borders; small terminals clamp it. The
    /// list never asks for fewer than ten, so its padding, and with it the filter's caret row,
    /// stays put while a filter empties the list (COM-1).
    pub(crate) fn preferred_rows(&self, width: u16) -> u16 {
        match &self.shown {
            Shown::Permissions(panel) => panel.preferred_rows(width),
            Shown::Configuration(summary) => summary.preferred_rows(),
            Shown::Pages => u16::try_from(self.pages().len())
                .unwrap_or(u16::MAX)
                .max(1)
                .saturating_add(8)
                .max(10),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Drawer, Page};
    use crate::surface::SurfaceId;

    fn filtered(text: &str) -> Drawer {
        let mut drawer = Drawer::opened_from(SurfaceId::Composer);
        for character in text.chars() {
            drawer.filter_mut().expect("list filter").insert(character);
        }
        drawer.reclamp();
        drawer
    }

    /// DRW-3: a page is found by any part of its name, and by nothing else.
    #[test]
    fn a_page_is_found_by_its_name_and_a_slash_is_a_character() {
        for typed in ["conf", "config", "Configuration", "FIGUR"] {
            assert_eq!(
                filtered(typed).pages(),
                vec![Page::Configuration],
                "typed {typed:?}"
            );
        }
        for typed in ["/config", "/", "settings", "new"] {
            assert!(filtered(typed).pages().is_empty(), "typed {typed:?}");
        }
        assert_eq!(filtered("").pages(), Page::ALL.to_vec());
    }

    /// A filter that admits nothing chooses nothing, rather than a stale row.
    #[test]
    fn a_filter_matching_nothing_leaves_no_choice() {
        let drawer = filtered("nothing-matches-this");
        assert!(drawer.pages().is_empty());
        assert_eq!(drawer.chosen_page(), None);
    }

    /// The choice stops at the ends rather than wrapping.
    #[test]
    fn stepping_stops_at_the_ends_of_the_pages() {
        let mut drawer = filtered("");
        assert!(!drawer.step(false));
        for page in Page::ALL.into_iter().skip(1) {
            assert!(drawer.step(true));
            assert_eq!(drawer.chosen_page(), Some(page));
        }
        assert!(!drawer.step(true));
    }
}
