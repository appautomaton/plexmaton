//! The single translator from terminal events to typed intents.
//!
//! Invariants INV-1 to INV-9 are defined in `.agents/specs/interaction-routing.md`; the tests below
//! cite them by number. Nothing else in the workspace may accept a `crossterm::event::Event`.

// Ratatui re-exports the Crossterm generation it was built against. Depending on that re-export
// rather than declaring Crossterm here keeps the terminal types single-sourced.
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::{
    intent::{Direction, PointerIntent, ScrollDirection, TextIntent, TuiIntent},
    surface::{KeyboardFocus, Point, SurfaceId, SurfaceTree},
};

/// Read-only view facts the router reads but does not own.
///
/// Copying focus or the dismissible stack into the router would create the second source of truth
/// that `AGENTS.md` rejects, so they are borrowed per event instead.
#[derive(Clone, Copy, Debug)]
pub struct RouterContext<'a> {
    /// Geometry and z-order the renderer laid out.
    pub surfaces: &'a SurfaceTree,
    /// Where typed text would go right now.
    pub focus: KeyboardFocus,
    /// Whether a dismissible layer is open above the workspace.
    pub dismissible: bool,
}

/// Why a terminal event produced no intent.
///
/// Ignoring is a named outcome rather than a fallthrough, so a key that quietly does nothing is
/// visible in a test instead of being indistinguishable from a routing defect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ignored {
    /// A key release or a non-press frame from a terminal that reports both.
    KeyRelease,
    /// The input has no binding in the current focus.
    Unbound,
    /// The pointer was over no registered surface.
    OutsideWorkspace,
    /// A drag or release arrived with no capture held.
    NoCapture,
    /// `Escape` with no drag to cancel and nothing dismissible.
    NothingToDismiss,
    /// The wheel was over the workspace, but nothing under it had anywhere to scroll.
    NothingScrollable,
    /// The modifier escape hatch: this event belongs to the terminal's own selection.
    TerminalSelection,
}

/// Outcome of translating one terminal event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Routed {
    /// The event asked the workspace for something.
    Intent(TuiIntent),
    /// The event asked for nothing, for the named reason.
    Ignored(Ignored),
}

/// Owns pointer capture and nothing else.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Router {
    capture: Option<SurfaceId>,
}

impl Router {
    /// Translates one terminal event into at most one intent.
    pub fn translate(&mut self, event: &Event, context: &RouterContext<'_>) -> Routed {
        match *event {
            Event::Key(key) => self.on_key(key, context),
            Event::Mouse(mouse) => self.on_mouse(mouse, context),
            Event::Resize(width, height) => {
                Routed::Intent(TuiIntent::TerminalResized { width, height })
            }
            Event::FocusGained | Event::FocusLost | Event::Paste(_) => {
                Routed::Ignored(Ignored::Unbound)
            }
        }
    }

    /// Returns the surface currently holding pointer capture, if any.
    #[must_use]
    pub const fn capture(&self) -> Option<SurfaceId> {
        self.capture
    }

    fn on_key(&mut self, key: KeyEvent, context: &RouterContext<'_>) -> Routed {
        // A repeat frame is a press the user is still holding; a release is not an input at all.
        if key.kind == KeyEventKind::Release {
            return Routed::Ignored(Ignored::KeyRelease);
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Routed::Intent(TuiIntent::Quit);
        }

        match key.code {
            KeyCode::Esc => self.escape(context),
            KeyCode::BackTab => Routed::Intent(TuiIntent::CycleFocus(Direction::Backward)),
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                Routed::Intent(TuiIntent::CycleFocus(Direction::Backward))
            }
            KeyCode::Tab => Routed::Intent(TuiIntent::CycleFocus(Direction::Forward)),
            _ => match context.focus {
                KeyboardFocus::TextInput => text_key(key),
                KeyboardFocus::Navigation => navigation_key(key, context),
            },
        }
    }

    /// Resolves exactly one layer per press: an active drag, then a dismissible layer, then nothing.
    ///
    /// `Escape` never quits. It is the key people press to back out of a mistake, and the last
    /// press of that habit must not be the one that ends the session.
    fn escape(&mut self, context: &RouterContext<'_>) -> Routed {
        if let Some(surface) = self.capture.take() {
            return Routed::Intent(TuiIntent::Pointer(PointerIntent::Cancel { surface }));
        }
        if context.dismissible {
            return Routed::Intent(TuiIntent::Dismiss);
        }
        Routed::Ignored(Ignored::NothingToDismiss)
    }

    fn on_mouse(&mut self, mouse: MouseEvent, context: &RouterContext<'_>) -> Routed {
        // The alternate screen is ours, so the terminal's own selection needs a reserved modifier.
        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
            return Routed::Ignored(Ignored::TerminalSelection);
        }

        let at = Point {
            x: mouse.column,
            y: mouse.row,
        };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.press(at, context),
            MouseEventKind::Drag(MouseButton::Left) => self
                .capture
                .map_or(Routed::Ignored(Ignored::NoCapture), |surface| {
                    Routed::Intent(TuiIntent::Pointer(PointerIntent::Drag { surface, at }))
                }),
            MouseEventKind::Up(MouseButton::Left) => {
                self.capture
                    .take()
                    .map_or(Routed::Ignored(Ignored::NoCapture), |surface| {
                        Routed::Intent(TuiIntent::Pointer(PointerIntent::Release { surface, at }))
                    })
            }
            // A wheel is not part of the drag gesture. Capturing it would freeze scrolling
            // everywhere else for as long as a resize handle is held.
            MouseEventKind::ScrollUp => scroll(at, ScrollDirection::Up, context),
            MouseEventKind::ScrollDown => scroll(at, ScrollDirection::Down, context),
            _ => Routed::Ignored(Ignored::Unbound),
        }
    }

    fn press(&mut self, at: Point, context: &RouterContext<'_>) -> Routed {
        let Some(surface) = context.surfaces.hit_test(at) else {
            return Routed::Ignored(Ignored::OutsideWorkspace);
        };
        self.capture = Some(surface);
        Routed::Intent(TuiIntent::Pointer(PointerIntent::Press { surface, at }))
    }
}

/// Resolves a wheel event against viewports rather than against geometry.
///
/// Eligibility is whether a viewport can move, so the wheel falls through a surface with nothing to
/// scroll and reaches the one beneath it. A viewport that is merely at its boundary is still
/// eligible and still consumes the event: a gesture whose target changes with scroll position is
/// the spatial-memory failure the contract exists to prevent (D-006).
fn scroll(at: Point, direction: ScrollDirection, context: &RouterContext<'_>) -> Routed {
    if let Some(surface) = context.surfaces.wheel_target(at) {
        return Routed::Intent(TuiIntent::Scroll { surface, direction });
    }
    // Two different facts, and a test should be able to tell them apart.
    if context.surfaces.hit_test(at).is_none() {
        return Routed::Ignored(Ignored::OutsideWorkspace);
    }
    Routed::Ignored(Ignored::NothingScrollable)
}

/// Keys addressed to the one visible cursor.
fn text_key(key: KeyEvent) -> Routed {
    let commanded = key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    match key.code {
        // `Shift` stays allowed: it is how a capital letter arrives, not a command modifier.
        KeyCode::Char(character) if !commanded => {
            Routed::Intent(TuiIntent::Text(TextIntent::Insert(character)))
        }
        KeyCode::Backspace if !commanded => {
            Routed::Intent(TuiIntent::Text(TextIntent::DeleteBackward))
        }
        KeyCode::Enter
            if key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
        {
            Routed::Intent(TuiIntent::Text(TextIntent::Newline))
        }
        KeyCode::Enter => Routed::Intent(TuiIntent::Text(TextIntent::Submit)),
        _ => Routed::Ignored(Ignored::Unbound),
    }
}

/// Keys addressed to a navigational surface, where no cursor exists.
fn navigation_key(key: KeyEvent, context: &RouterContext<'_>) -> Routed {
    if !key.modifiers.is_empty() {
        return Routed::Ignored(Ignored::Unbound);
    }
    match key.code {
        // Quitting must not be the way a dismissible layer gets closed.
        KeyCode::Char('q') if !context.dismissible => Routed::Intent(TuiIntent::Quit),
        KeyCode::Down | KeyCode::Char('j') => {
            Routed::Intent(TuiIntent::MoveSelection(Direction::Forward))
        }
        KeyCode::Up | KeyCode::Char('k') => {
            Routed::Intent(TuiIntent::MoveSelection(Direction::Backward))
        }
        _ => Routed::Ignored(Ignored::Unbound),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{
        crossterm::event::{
            Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton,
            MouseEvent, MouseEventKind,
        },
        layout::Rect,
    };

    use super::{Ignored, KeyboardFocus, Routed, Router, RouterContext};
    use crate::{
        intent::{Direction, PointerIntent, ScrollDirection, TextIntent, TuiIntent},
        surface::{Point, Surface, SurfaceId, SurfaceKind, SurfaceTree, Viewport},
    };

    // Two real identities in a covering arrangement. The router's grammar does not depend on which
    // regions these are, only that one sits above the other.
    const PANEL: SurfaceId = SurfaceId::Transcript;
    const OVERLAY: SurfaceId = SurfaceId::Notices;

    fn tree() -> SurfaceTree {
        let mut tree = SurfaceTree::default();
        for (id, bounds, z_index) in [
            (PANEL, Rect::new(0, 0, 40, 20), 1),
            (OVERLAY, Rect::new(10, 2, 10, 6), 2),
        ] {
            tree.insert(Surface {
                id,
                bounds,
                z_index,
                kind: SurfaceKind::Panel,
                // Content taller than the region, so the wheel has somewhere to go. A viewport
                // that cannot move is a separate case with its own test below.
                viewport: Some(Viewport {
                    content_rows: 100,
                    visible_rows: bounds.height,
                    offset: 0,
                }),
            })
            .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
        }
        tree
    }

    fn context(
        surfaces: &SurfaceTree,
        focus: KeyboardFocus,
        dismissible: bool,
    ) -> RouterContext<'_> {
        RouterContext {
            surfaces,
            focus,
            dismissible,
        }
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn press(x: u16, y: u16) -> Event {
        mouse(MouseEventKind::Down(MouseButton::Left), x, y)
    }

    /// INV-1: a translation is either one intent or a named reason for none.
    #[test]
    fn every_terminal_event_is_translated_or_named_as_ignored() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();

        let events = [
            key(KeyCode::Char('q'), KeyModifiers::NONE),
            key(KeyCode::F(7), KeyModifiers::NONE),
            Event::Resize(100, 40),
            Event::FocusGained,
            Event::Paste("pasted".to_owned()),
            press(1, 1),
            mouse(MouseEventKind::Moved, 1, 1),
            mouse(MouseEventKind::ScrollLeft, 1, 1),
        ];

        // The point is exhaustiveness: no event may fall through without an outcome, and an
        // ignored event must say why rather than looking like a routing defect.
        for event in events {
            match router.translate(&event, &context) {
                Routed::Intent(_) | Routed::Ignored(_) => {}
            }
        }
        assert_eq!(
            router.translate(&key(KeyCode::F(7), KeyModifiers::NONE), &context),
            Routed::Ignored(Ignored::Unbound)
        );
    }

    /// INV-2: the same key is text or a command depending only on where the cursor is.
    #[test]
    fn printable_keys_follow_the_cursor() {
        let surfaces = tree();
        let mut router = Router::default();

        assert_eq!(
            router.translate(
                &key(KeyCode::Char('j'), KeyModifiers::NONE),
                &context(&surfaces, KeyboardFocus::TextInput, false)
            ),
            Routed::Intent(TuiIntent::Text(TextIntent::Insert('j')))
        );
        assert_eq!(
            router.translate(
                &key(KeyCode::Char('j'), KeyModifiers::NONE),
                &context(&surfaces, KeyboardFocus::Navigation, false)
            ),
            Routed::Intent(TuiIntent::MoveSelection(Direction::Forward))
        );
        // A capital letter is still text: Shift is not a command modifier.
        assert_eq!(
            router.translate(
                &key(KeyCode::Char('J'), KeyModifiers::SHIFT),
                &context(&surfaces, KeyboardFocus::TextInput, false)
            ),
            Routed::Intent(TuiIntent::Text(TextIntent::Insert('J')))
        );
    }

    /// INV-3: hover routing picks the topmost surface and cannot move focus.
    #[test]
    fn wheel_routes_by_hover_and_never_changes_focus() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();

        assert_eq!(
            router.translate(&mouse(MouseEventKind::ScrollDown, 12, 4), &context),
            Routed::Intent(TuiIntent::Scroll {
                surface: OVERLAY,
                direction: ScrollDirection::Down,
            })
        );
        assert_eq!(
            router.translate(&mouse(MouseEventKind::ScrollUp, 2, 4), &context),
            Routed::Intent(TuiIntent::Scroll {
                surface: PANEL,
                direction: ScrollDirection::Up,
            })
        );
        assert_eq!(
            router.translate(&mouse(MouseEventKind::ScrollUp, 99, 99), &context),
            Routed::Ignored(Ignored::OutsideWorkspace)
        );
        assert_eq!(router.capture(), None, "hover must not take capture");
    }

    /// INV-3 and D-006: eligibility is whether a viewport can move, not what is on top.
    ///
    /// The two halves are deliberately different. A surface with nothing to scroll is transparent
    /// to the wheel, so the event reaches what is beneath it; a surface that is merely *at* its
    /// boundary still consumes it, because a gesture whose target changes with scroll position is
    /// the spatial-memory failure the contract exists to prevent.
    #[test]
    fn the_wheel_falls_through_what_cannot_scroll_and_stops_at_what_is_merely_exhausted() {
        let mut surfaces = tree();
        let mut router = Router::default();
        let over_the_overlay = mouse(MouseEventKind::ScrollDown, 12, 4);

        // Nothing to scroll: the topmost surface is transparent to the wheel.
        surfaces.set_viewport(
            OVERLAY,
            Viewport {
                content_rows: 2,
                visible_rows: 6,
                offset: 0,
            },
        );
        assert_eq!(
            router.translate(
                &over_the_overlay,
                &context(&surfaces, KeyboardFocus::Navigation, false)
            ),
            Routed::Intent(TuiIntent::Scroll {
                surface: PANEL,
                direction: ScrollDirection::Down,
            }),
            "the wheel reached the panel underneath"
        );

        // Scrollable but already at the end: still the target, and the event stops here.
        surfaces.set_viewport(
            OVERLAY,
            Viewport {
                content_rows: 40,
                visible_rows: 6,
                offset: 34,
            },
        );
        assert_eq!(
            router.translate(
                &over_the_overlay,
                &context(&surfaces, KeyboardFocus::Navigation, false)
            ),
            Routed::Intent(TuiIntent::Scroll {
                surface: OVERLAY,
                direction: ScrollDirection::Down,
            }),
            "an exhausted viewport consumes the wheel rather than passing it down"
        );
    }

    #[test]
    fn a_wheel_over_the_workspace_with_nothing_to_scroll_says_so() {
        let mut surfaces = tree();
        for id in [PANEL, OVERLAY] {
            surfaces.set_viewport(
                id,
                Viewport {
                    content_rows: 1,
                    visible_rows: 6,
                    offset: 0,
                },
            );
        }
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();

        assert_eq!(
            router.translate(&mouse(MouseEventKind::ScrollDown, 12, 4), &context),
            Routed::Ignored(Ignored::NothingScrollable),
            "over the workspace, but nothing had anywhere to go"
        );
        assert_eq!(
            router.translate(&mouse(MouseEventKind::ScrollDown, 99, 99), &context),
            Routed::Ignored(Ignored::OutsideWorkspace),
            "and that is a different fact from being outside it"
        );
    }

    /// INV-4: a drag stays on its surface even when the pointer leaves it.
    #[test]
    fn capture_keeps_the_drag_on_its_surface() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();

        assert_eq!(
            router.translate(&press(12, 4), &context),
            Routed::Intent(TuiIntent::Pointer(PointerIntent::Press {
                surface: OVERLAY,
                at: Point { x: 12, y: 4 },
            }))
        );
        // Over the panel, and then off the workspace entirely: both belong to the overlay.
        for (x, y) in [(2_u16, 18_u16), (200, 200)] {
            assert_eq!(
                router.translate(
                    &mouse(MouseEventKind::Drag(MouseButton::Left), x, y),
                    &context
                ),
                Routed::Intent(TuiIntent::Pointer(PointerIntent::Drag {
                    surface: OVERLAY,
                    at: Point { x, y },
                }))
            );
        }
    }

    /// INV-4: the wheel keeps hover routing while a drag is held.
    #[test]
    fn wheel_is_not_captured_by_a_drag() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();
        router.translate(&press(12, 4), &context);

        assert_eq!(
            router.translate(&mouse(MouseEventKind::ScrollDown, 2, 18), &context),
            Routed::Intent(TuiIntent::Scroll {
                surface: PANEL,
                direction: ScrollDirection::Down,
            })
        );
        assert_eq!(router.capture(), Some(OVERLAY), "the drag is still held");
    }

    /// INV-5: releasing twice cannot produce two gestures.
    #[test]
    fn capture_is_released_exactly_once() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();
        router.translate(&press(12, 4), &context);

        let release = mouse(MouseEventKind::Up(MouseButton::Left), 12, 5);
        assert_eq!(
            router.translate(&release, &context),
            Routed::Intent(TuiIntent::Pointer(PointerIntent::Release {
                surface: OVERLAY,
                at: Point { x: 12, y: 5 },
            }))
        );
        assert_eq!(
            router.translate(&release, &context),
            Routed::Ignored(Ignored::NoCapture)
        );
        assert_eq!(
            router.translate(
                &mouse(MouseEventKind::Drag(MouseButton::Left), 12, 6),
                &context
            ),
            Routed::Ignored(Ignored::NoCapture)
        );
    }

    /// INV-6: one layer per press, and never a quit.
    #[test]
    fn escape_resolves_one_layer_per_press() {
        let surfaces = tree();
        let mut router = Router::default();
        let with_layer = context(&surfaces, KeyboardFocus::Navigation, true);
        router.translate(&press(12, 4), &with_layer);

        let escape = key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            router.translate(&escape, &with_layer),
            Routed::Intent(TuiIntent::Pointer(PointerIntent::Cancel {
                surface: OVERLAY
            })),
            "the drag is the topmost layer"
        );
        assert_eq!(
            router.translate(&escape, &with_layer),
            Routed::Intent(TuiIntent::Dismiss),
            "the dismissible layer is next, not both at once"
        );

        let bare = context(&surfaces, KeyboardFocus::Navigation, false);
        assert_eq!(
            router.translate(&escape, &bare),
            Routed::Ignored(Ignored::NothingToDismiss)
        );
    }

    /// INV-7: quitting is deliberate, and typing `q` is typing.
    #[test]
    fn quit_is_explicit_and_unreachable_while_typing() {
        let surfaces = tree();
        let mut router = Router::default();

        assert_eq!(
            router.translate(
                &key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                &context(&surfaces, KeyboardFocus::TextInput, true)
            ),
            Routed::Intent(TuiIntent::Quit)
        );
        assert_eq!(
            router.translate(
                &key(KeyCode::Char('q'), KeyModifiers::NONE),
                &context(&surfaces, KeyboardFocus::TextInput, false)
            ),
            Routed::Intent(TuiIntent::Text(TextIntent::Insert('q')))
        );
        assert_eq!(
            router.translate(
                &key(KeyCode::Char('q'), KeyModifiers::NONE),
                &context(&surfaces, KeyboardFocus::Navigation, true)
            ),
            Routed::Ignored(Ignored::Unbound),
            "a dismissible layer must be closed, not escaped by quitting"
        );
        assert_eq!(
            router.translate(
                &key(KeyCode::Char('q'), KeyModifiers::NONE),
                &context(&surfaces, KeyboardFocus::Navigation, false)
            ),
            Routed::Intent(TuiIntent::Quit)
        );
    }

    /// INV-8: `Shift` hands the gesture back to the terminal.
    #[test]
    fn shift_leaves_pointer_events_to_the_terminal() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();

        let shifted = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 4,
            modifiers: KeyModifiers::SHIFT,
        });
        assert_eq!(
            router.translate(&shifted, &context),
            Routed::Ignored(Ignored::TerminalSelection)
        );
        assert_eq!(router.capture(), None, "capture must not be taken");
    }

    /// INV-9: geometry reaches the workspace as an intent like anything else.
    #[test]
    fn resize_is_an_intent() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();

        assert_eq!(
            router.translate(&Event::Resize(132, 40), &context),
            Routed::Intent(TuiIntent::TerminalResized {
                width: 132,
                height: 40,
            })
        );
    }

    #[test]
    fn key_releases_do_not_act_twice() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();
        let release = Event::Key(KeyEvent::new_with_kind_and_state(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
            KeyEventState::NONE,
        ));

        assert_eq!(
            router.translate(&release, &context),
            Routed::Ignored(Ignored::KeyRelease)
        );
    }

    #[test]
    fn tab_cycles_focus_from_either_side_of_the_cursor() {
        let surfaces = tree();
        let mut router = Router::default();

        for focus in [KeyboardFocus::Navigation, KeyboardFocus::TextInput] {
            let context = context(&surfaces, focus, false);
            assert_eq!(
                router.translate(&key(KeyCode::Tab, KeyModifiers::NONE), &context),
                Routed::Intent(TuiIntent::CycleFocus(Direction::Forward))
            );
            assert_eq!(
                router.translate(&key(KeyCode::BackTab, KeyModifiers::SHIFT), &context),
                Routed::Intent(TuiIntent::CycleFocus(Direction::Backward))
            );
        }
    }

    #[test]
    fn enter_submits_and_shift_enter_breaks_the_line() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::TextInput, false);
        let mut router = Router::default();

        assert_eq!(
            router.translate(&key(KeyCode::Enter, KeyModifiers::NONE), &context),
            Routed::Intent(TuiIntent::Text(TextIntent::Submit))
        );
        assert_eq!(
            router.translate(&key(KeyCode::Enter, KeyModifiers::SHIFT), &context),
            Routed::Intent(TuiIntent::Text(TextIntent::Newline))
        );
    }

    #[test]
    fn a_press_outside_every_surface_takes_no_capture() {
        let surfaces = SurfaceTree::default();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();

        assert_eq!(
            router.translate(&press(4, 4), &context),
            Routed::Ignored(Ignored::OutsideWorkspace)
        );
        assert_eq!(router.capture(), None);
    }
}
