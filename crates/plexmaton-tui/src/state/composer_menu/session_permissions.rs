//! `/permissions` in the composer menu: the Session's grants as rows under their panel's
//! description, reviewed and confirmed in place, and withdrawn with the draft (CMC-3, PER-7).
use super::{HEADING_LINES, Listing, MenuRow};
use crate::state::{
    ViewState,
    permissions::{PermissionChoice, PermissionPanel, PermissionPlace},
    wrap_line,
};

impl ViewState {
    /// The lines above a listing's rows: the Session permissions panel's description, wrapped
    /// behind the rows' two-cell marker column and capped, so a review reads before its
    /// confirmation (PER-7). `width` is the menu's inner width.
    pub(crate) fn menu_heading(&self, width: u16) -> Vec<String> {
        if self.menu_listing() != Some(Listing::Permissions) {
            return Vec::new();
        }
        let description = self.composer_menu.permissions.as_ref().map_or_else(
            || vec!["Loading permissions…".to_owned()],
            PermissionPanel::description,
        );
        let mut lines: Vec<String> = description
            .iter()
            .flat_map(|line| wrap_line(line, usize::from(width.saturating_sub(2).max(1))))
            .collect();
        if lines.len() > HEADING_LINES {
            lines.truncate(HEADING_LINES);
            if let Some(last) = lines.last_mut() {
                last.push('…');
            }
        }
        lines
    }

    /// The keys on the menu's last row; the Session permissions panel names its own phase.
    pub(crate) fn menu_keys(&self) -> String {
        match (self.menu_listing(), self.composer_menu.permissions.as_ref()) {
            (Some(Listing::Permissions), Some(panel)) => format!(" {}", panel.hint()),
            (Some(listing), _) => listing.keys().to_owned(),
            (None, _) => String::new(),
        }
    }

    /// Drops a Session permissions panel whose rows have no home: the draft stopped asking for
    /// them and no change is with the owner. The composition root reads the withdrawal (PER-7).
    pub(super) fn drop_unlisted_permissions(&mut self) -> bool {
        if self.menu_listing() == Some(Listing::Permissions)
            || self
                .composer_menu
                .permissions
                .as_ref()
                .is_none_or(PermissionPanel::is_submitting)
        {
            return false;
        }
        self.composer_menu.permissions = None;
        true
    }

    /// Makes room for the Session's permissions behind `/permissions`; a panel already there
    /// keeps what the owner projected.
    pub(crate) fn open_session_permissions(&mut self) {
        if self.composer_menu.permissions.is_none() {
            self.composer_menu.permissions =
                Some(PermissionPanel::loading(PermissionPlace::Session));
        }
        self.sync_composer_menu();
        self.touch();
    }

    /// Acts on a Session permission row: a review shows its confirmation with `Back` under the
    /// marker, a confirmation leaves as the reviewed intent, `Back` returns (PER-7).
    pub(crate) fn activate_menu_permission(
        &mut self,
        choice: &PermissionChoice,
    ) -> Option<plexmaton_core::PermissionIntent> {
        let panel = self.composer_menu.permissions.as_mut()?;
        let intent = panel.activate(choice);
        let text = self.composer().text().to_owned();
        let cursor = self.composer().cursor();
        self.composer_menu.sync(&text, cursor);
        if matches!(choice, PermissionChoice::Review(_)) {
            self.composer_menu
                .choose(&text, cursor, MenuRow::Permission(PermissionChoice::Back));
        }
        self.touch();
        intent
    }

    /// `Escape` on a Session permission review returns to its rows, one layer per press.
    pub(crate) fn menu_permission_back(&mut self) -> bool {
        let backed = self
            .composer_menu
            .permissions
            .as_mut()
            .is_some_and(PermissionPanel::back);
        if backed {
            self.sync_composer_menu();
            self.touch();
        }
        backed
    }
}
