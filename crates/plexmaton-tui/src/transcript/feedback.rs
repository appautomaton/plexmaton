//! The same entry-local feedback rows supply height measurement and visible composition.

use ratatui::{
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::{
    AgentView, Palette, RetryAction, Role, TranscriptEntryView, content, state::FeedbackPlacement,
};

pub(super) struct EntryFeedback {
    pub(super) before: Vec<Line<'static>>,
    pub(super) after: Vec<Line<'static>>,
}

impl EntryFeedback {
    pub(super) fn new(
        agent: &AgentView,
        item: &TranscriptEntryView,
        palette: &Palette,
        hovered: Option<RetryAction>,
    ) -> Self {
        let mut before = Vec::new();
        let mut after = Vec::new();
        if agent
            .retry
            .as_ref()
            .is_some_and(|actions| &actions.error_item == item.id())
        {
            let style = |action| {
                palette.style(if hovered == Some(action) {
                    Role::Accent
                } else {
                    Role::Muted
                })
            };
            after.push(Line::from(vec![
                Span::styled("[ Retry ]", style(RetryAction::Retry)),
                Span::raw("   "),
                Span::styled("[ Edit & retry ]", style(RetryAction::EditRetry)),
                Span::styled(" · r / e", palette.style(Role::Muted)),
            ]));
            after.push(Line::default());
        }
        if item.saved_project_permission().is_some() {
            after.extend(crate::content_permissions::saved_permission_lines(palette));
        }
        if let Some((place, note)) = agent.note_for(item.id()) {
            let destination = match place {
                FeedbackPlacement::Before => &mut before,
                FeedbackPlacement::After => &mut after,
            };
            destination.extend(content::note_lines(note, palette));
        }
        Self { before, after }
    }

    pub(super) fn rows(&self, width: u16) -> (usize, usize) {
        let rows = |lines: &[Line<'static>]| {
            Paragraph::new(lines.to_vec())
                .wrap(Wrap { trim: false })
                .line_count(width)
        };
        let leading = rows(&self.before);
        (leading, leading.saturating_add(rows(&self.after)))
    }
}
