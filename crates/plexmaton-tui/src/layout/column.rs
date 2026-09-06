//! The agent column beside the primary conversation.

use ratatui::layout::{Constraint, Layout, Rect};

use super::BodyRegions;

/// The agent column on the left and the conversation beside it (ui-ux §layout classes).
///
/// Tool, artifact and mail facts now live in each owning conversation, so the rail keeps the whole
/// column and reports only compact counts beside each agent.
pub(super) fn beside_conversation(body: Rect, rail: u16) -> BodyRegions {
    let [column, transcript] =
        Layout::horizontal([Constraint::Length(rail), Constraint::Min(24)]).areas(body);
    BodyRegions {
        agents: Some(column),
        transcript: Some(transcript),
        inspector: None,
        inspector_floats: false,
        composer: Rect::default(),
        decision: None,
        drawer: None,
    }
}
