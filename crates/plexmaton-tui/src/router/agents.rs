//! The Narrow full-region Agents navigator's closed keyboard grammar.

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::{Ignored, Routed, Router, RouterContext};
use crate::{Direction, InspectorIntent, TuiIntent};

impl Router {
    /// The complete non-global grammar while Agents owns the body.
    pub(super) fn agents_key(&mut self, key: KeyEvent, context: &RouterContext<'_>) -> Routed {
        if !key.modifiers.is_empty() {
            return Routed::Ignored(Ignored::Unbound);
        }
        match key.code {
            KeyCode::Esc => self.escape(context),
            KeyCode::Enter => Routed::Intent(TuiIntent::Inspector(InspectorIntent::Open)),
            KeyCode::Down | KeyCode::Char('j') => {
                Routed::Intent(TuiIntent::MoveSelection(Direction::Forward))
            }
            KeyCode::Up | KeyCode::Char('k') => {
                Routed::Intent(TuiIntent::MoveSelection(Direction::Backward))
            }
            _ => Routed::Ignored(Ignored::Unbound),
        }
    }
}
