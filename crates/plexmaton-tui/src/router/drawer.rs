//! The Drawer's single-line grammar, using the shared router's capture and text translation.

use super::{Ignored, Routed, Router, RouterContext, text_key};
use crate::{Direction, KeyboardFocus, SelectionIntent, TuiIntent, intent::DrawerIntent};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl Router {
    /// The Drawer consumes its navigation grammar and keeps its filter single-line (DRW-3).
    pub(super) fn drawer_key(&mut self, key: KeyEvent, context: &RouterContext<'_>) -> Routed {
        let typing = context.focus == KeyboardFocus::TextInput;
        match key.code {
            KeyCode::Char('y') if typing && key.modifiers.contains(KeyModifiers::CONTROL) => {
                Routed::Intent(TuiIntent::Selection(SelectionIntent::Copy))
            }
            KeyCode::Esc => self.escape(context),
            KeyCode::Up => {
                Routed::Intent(TuiIntent::Drawer(DrawerIntent::Step(Direction::Backward)))
            }
            KeyCode::Down => {
                Routed::Intent(TuiIntent::Drawer(DrawerIntent::Step(Direction::Forward)))
            }
            KeyCode::Char('k') if !typing && key.modifiers.is_empty() => {
                Routed::Intent(TuiIntent::Drawer(DrawerIntent::Step(Direction::Backward)))
            }
            KeyCode::Char('j') if !typing && key.modifiers.is_empty() => {
                Routed::Intent(TuiIntent::Drawer(DrawerIntent::Step(Direction::Forward)))
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                Routed::Intent(TuiIntent::Drawer(DrawerIntent::Choose))
            }
            KeyCode::Enter => Routed::Ignored(Ignored::Unbound),
            KeyCode::Char('j') if key.modifiers == KeyModifiers::CONTROL => {
                Routed::Ignored(Ignored::Unbound)
            }

            _ if typing => text_key(key),
            _ => Routed::Ignored(Ignored::Unbound),
        }
    }
}
