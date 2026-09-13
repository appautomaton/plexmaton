//! The tree's blocking navigation/editor grammar shares the one Router and capture owner.

use super::{Ignored, Routed, Router, RouterContext, text_key};
use crate::{Direction, SurfaceId, TreeIntent, TuiIntent};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub(super) fn paste(text: &str, context: &RouterContext<'_>) -> Routed {
    if context.tree_text_input && context.focused == Some(SurfaceId::ConversationTree) {
        Routed::Intent(TuiIntent::Tree(TreeIntent::EditInput(
            crate::TextIntent::Paste(text.to_owned()),
        )))
    } else {
        Routed::Ignored(Ignored::Unbound)
    }
}

impl Router {
    pub(super) fn tree_key(&mut self, key: KeyEvent, context: &RouterContext<'_>) -> Routed {
        if key.code == KeyCode::Esc {
            if self.capture.is_some() {
                return self.escape(context);
            }
            return Routed::Intent(TuiIntent::Tree(if context.tree_editing {
                TreeIntent::CancelEdit
            } else {
                TreeIntent::Close
            }));
        }
        // Below minimum size only the explicit close path and process-wide chords remain usable.
        if context.surfaces.get(SurfaceId::ConversationTree).is_none() {
            return Routed::Ignored(Ignored::Unbound);
        }
        let pressed = key.kind == KeyEventKind::Press;
        if context.tree_editing {
            if key.code == KeyCode::Enter {
                return if key.modifiers.is_empty() && pressed {
                    Routed::Intent(TuiIntent::Tree(TreeIntent::SubmitEdit))
                } else {
                    Routed::Ignored(Ignored::Unbound)
                };
            }
            return if context.tree_text_input {
                match text_key(key) {
                    Routed::Intent(TuiIntent::Text(intent)) => {
                        Routed::Intent(TuiIntent::Tree(TreeIntent::EditInput(intent)))
                    }
                    _ => Routed::Ignored(Ignored::Unbound),
                }
            } else {
                Routed::Ignored(Ignored::Unbound)
            };
        }
        let plain = key.modifiers.is_empty();
        let intent = match key.code {
            KeyCode::Up | KeyCode::Char('k') if plain => TreeIntent::Move(Direction::Backward),
            KeyCode::Down | KeyCode::Char('j') if plain => TreeIntent::Move(Direction::Forward),
            KeyCode::Home if plain => TreeIntent::Home,
            KeyCode::End if plain => TreeIntent::End,
            KeyCode::Enter if plain && pressed => TreeIntent::Navigate,
            KeyCode::Char('b') if plain && pressed => TreeIntent::ToggleBranches,
            KeyCode::Char('f') if plain && pressed => TreeIntent::ToggleFold,
            KeyCode::Char('r') if plain && pressed => TreeIntent::Refresh,
            KeyCode::Char('y') if pressed && (plain || key.modifiers == KeyModifiers::CONTROL) => {
                TreeIntent::CopySource
            }
            KeyCode::Char('n') if plain && pressed => TreeIntent::RenameHead,
            KeyCode::Char('l') if plain && pressed => TreeIntent::EditLabel,
            KeyCode::Char('x') if plain && pressed => TreeIntent::AbandonHead,
            _ => return Routed::Ignored(Ignored::Unbound),
        };
        Routed::Intent(TuiIntent::Tree(intent))
    }
}
