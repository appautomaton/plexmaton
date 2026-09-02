//! The agent column: the list of sub-agents over the activity of the agent being looked at, in
//! one box on the left (D-014), with the conversation beside it.

use ratatui::layout::{Constraint, Layout, Rect};

use super::{BodyRegions, MIN_PANEL_HEIGHT};

/// The agent column on the left and the conversation beside it (D-014).
///
/// The column is one box holding the list of sub-agents above the activity — tools, artifacts and
/// mail — of the agent being looked at. The list takes the rows its agents need and the activity
/// takes the rest; a column too short for both keeps the list, because identity outranks detail.
pub(super) fn beside_conversation(body: Rect, rail: u16, sub_agents: usize) -> BodyRegions {
    let [column, transcript] =
        Layout::horizontal([Constraint::Length(rail), Constraint::Min(24)]).areas(body);
    let (agents, activity) = sidebar(column, sub_agents);
    BodyRegions {
        agents: Some(agents),
        transcript: Some(transcript),
        inspector: None,
        inspector_floats: false,
        composer: Rect::default(),
        activity,
    }
}

/// Splits the agent column between the list and the activity beneath it.
///
/// The list's rows are its top edge plus two per sub-agent — a label and its status wrap to two
/// on a column this wide — with a floor of three lines so an empty list can say so. The activity
/// section needs a divider, two lines and the bottom edge before it is worth registering.
fn sidebar(column: Rect, sub_agents: usize) -> (Rect, Option<Rect>) {
    let listed = u16::try_from(sub_agents)
        .unwrap_or(u16::MAX)
        .saturating_mul(2);
    let list_rows = listed.max(MIN_PANEL_HEIGHT).saturating_add(1);
    let activity_floor = MIN_PANEL_HEIGHT.saturating_add(1);
    if column.height < list_rows.saturating_add(activity_floor) {
        return (column, None);
    }
    let list_rows = list_rows.min(column.height / 2);
    let agents = Rect::new(column.x, column.y, column.width, list_rows);
    let activity = Rect::new(
        column.x,
        agents.bottom(),
        column.width,
        column.height.saturating_sub(list_rows),
    );
    (agents, Some(activity))
}
