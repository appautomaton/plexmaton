//! One menu choice for pointer and keyboard navigation, without acceptance (INV-3).
use super::{Direction, MenuRow, ViewState};

impl ViewState {
    pub(crate) fn close_composer_menu(&mut self) {
        self.composer_menu.effort_feedback = None;
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        if self.composer_menu.dismiss(&text, cursor) {
            self.touch();
        }
        // A dismissed listing has no destination for what the composition root is loading.
        if self.composer_menu.conversations.take().is_some()
            | self.composer_menu.permissions.take().is_some()
        {
            self.touch();
        }
    }

    /// Updates the menu's one navigation choice without accepting it or editing its query.
    pub(crate) fn choose_menu_row(&mut self, row: MenuRow) {
        if self.menu_chosen().as_ref() == Some(&row) {
            return;
        }
        let before = self.menu_chosen();
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        self.composer_menu.choose(&text, cursor, row);
        if self.menu_chosen() != before {
            self.touch();
        }
    }

    pub(crate) fn step_composer_menu(&mut self, direction: Direction) {
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        if self.composer_menu.step(&text, cursor, direction) {
            self.touch();
        }
    }

    /// The row `Enter` acts on.
    pub(crate) fn menu_chosen(&self) -> Option<MenuRow> {
        self.composer_menu.chosen().cloned()
    }
}
