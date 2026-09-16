//! The blocking approval surface's closed keyboard grammar.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{Ignored, Routed, Router, RouterContext};
use crate::{ApprovalIntent, Direction, TuiIntent};

impl Router {
    /// Approval owns its focused grammar; every granting key needs an explicit press.
    pub(super) fn approval_key(&mut self, key: KeyEvent, context: &RouterContext<'_>) -> Routed {
        match key.code {
            KeyCode::Esc => self.escape(context),
            KeyCode::Up | KeyCode::Char('k') => Routed::Intent(TuiIntent::Approval(
                ApprovalIntent::Move(Direction::Backward),
            )),
            KeyCode::Down | KeyCode::Char('j') => Routed::Intent(TuiIntent::Approval(
                ApprovalIntent::Move(Direction::Forward),
            )),
            KeyCode::Char(number @ '1'..='3')
                if key.modifiers.is_empty() && key.kind == KeyEventKind::Press =>
            {
                Routed::Intent(TuiIntent::Approval(ApprovalIntent::Shortcut(
                    number as u8 - b'0',
                )))
            }
            KeyCode::Enter if key.kind == KeyEventKind::Press => {
                Routed::Intent(TuiIntent::Approval(ApprovalIntent::Decide))
            }
            // The same chord that discloses a tool entry, doing the same thing to the request
            // one is asking about.
            KeyCode::Char('o' | 'O') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Routed::Intent(TuiIntent::Approval(ApprovalIntent::ToggleDetail))
            }
            KeyCode::BackTab => Routed::Intent(TuiIntent::CycleFocus(Direction::Backward)),
            KeyCode::Tab => Routed::Intent(TuiIntent::CycleFocus(Direction::Forward)),
            _ => Routed::Ignored(Ignored::Unbound),
        }
    }
}
