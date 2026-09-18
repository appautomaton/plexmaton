//! What a listing says when it has something to say instead of, or under, its rows.

use super::ViewState;
use crate::state::wrap_line;

/// Lines one status may wrap onto. A status says one thing; two rows hold it at 60 columns.
const STATUS_LINES: usize = 2;

impl ViewState {
    /// The status, wrapped to the rows it will actually draw.
    ///
    /// The status was the menu's one single-line holdout: `menu_heading` has always returned the
    /// lines it occupies, and both the height reservation and the draw budget count those. A
    /// sentence that had to fit one truncated row could say what went wrong but not what to do
    /// about it, and the instruction is the half a narrow terminal cut. Bounded like the heading,
    /// because a status is still a status.
    pub(crate) fn menu_status_lines(&self, width: u16) -> Vec<String> {
        let Some(status) = self.menu_status() else {
            return Vec::new();
        };
        let mut lines: Vec<String> = wrap_line(
            status.message(),
            usize::from(width.saturating_sub(2).max(1)),
        );
        if lines.len() > STATUS_LINES {
            lines.truncate(STATUS_LINES);
            if let Some(last) = lines.last_mut() {
                last.push('…');
            }
        }
        lines
    }
}
