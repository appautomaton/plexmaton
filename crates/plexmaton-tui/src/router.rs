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
    intent::{
        ApprovalIntent, AttentionIntent, CommandPaletteIntent, Direction, InspectorIntent,
        PointerIntent, ScrollDirection, SelectionIntent, SkillPickerIntent, TextIntent, TuiIntent,
    },
    state::Motion,
    surface::{KeyboardFocus, Point, SurfaceId, SurfaceTree, Viewport},
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
    /// Which surface holds keyboard focus, resolved against the frame that was drawn.
    ///
    /// `focus` says whether a cursor exists; this says where the user is. Both are needed because a
    /// navigation key means "move within the thing I am in", and there is more than one thing.
    pub focused: Option<SurfaceId>,
    /// Whether a dismissible layer is open above the workspace.
    pub dismissible: bool,
    /// Whether the user has a selection, which is a rung of the `Escape` ladder above that layer.
    pub selecting: bool,
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
    /// The surface addressed by the wheel or by an arrow had nowhere to scroll.
    NothingScrollable,
}

/// Outcome of translating one terminal event.
#[derive(Clone, Debug, Eq, PartialEq)]
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
            Event::FocusLost => self.capture.map_or(
                Routed::Intent(TuiIntent::Hover {
                    surface: None,
                    at: Point { x: 0, y: 0 },
                }),
                |surface| Routed::Intent(TuiIntent::Pointer(PointerIntent::Suspend { surface })),
            ),
            Event::Paste(ref text) if context.focus == KeyboardFocus::TextInput => {
                Routed::Intent(TuiIntent::Text(TextIntent::Paste(text.clone())))
            }
            Event::FocusGained | Event::Paste(_) => Routed::Ignored(Ignored::Unbound),
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
        // Both chords resolve before focus is consulted: under a cursor a control chord is never
        // text (INV-2), and neither key may mean something else somewhere (INV-7).
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => return Routed::Intent(TuiIntent::Quit),
                KeyCode::Char('c') => return Routed::Intent(TuiIntent::Interrupt),
                _ => {}
            }
        }

        // Its own chord, resolved before focus, so it opens from wherever the user is. `⌃P` is
        // unclaimed and reaches no text input: a control chord under a cursor is never the letter.
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('p')
            && context.focused != Some(SurfaceId::CommandPalette)
        {
            return Routed::Intent(TuiIntent::CommandPalette(CommandPaletteIntent::Open));
        }

        // The command list owns every non-global key while it is open, the same way an approval
        // does, because both block below (SURF-4). `Escape` still falls through to the ladder.
        if context.focused == Some(SurfaceId::CommandPalette) {
            return match key.code {
                KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Routed::Intent(TuiIntent::Selection(SelectionIntent::Copy))
                }
                KeyCode::Esc => self.escape(context),
                KeyCode::Up => Routed::Intent(TuiIntent::CommandPalette(
                    CommandPaletteIntent::Step(Direction::Backward),
                )),
                KeyCode::Down => Routed::Intent(TuiIntent::CommandPalette(
                    CommandPaletteIntent::Step(Direction::Forward),
                )),
                KeyCode::Enter => {
                    Routed::Intent(TuiIntent::CommandPalette(CommandPaletteIntent::Run))
                }
                _ => text_key(key),
            };
        }

        // The completion list keeps keyboard focus and the caret in the primary composer.
        if context.focused == Some(SurfaceId::Composer)
            && context.surfaces.get(SurfaceId::SkillPicker).is_some()
        {
            return match key.code {
                KeyCode::Esc => Routed::Intent(TuiIntent::SkillPicker(SkillPickerIntent::Close)),
                KeyCode::Up if key.modifiers.is_empty() => Routed::Intent(TuiIntent::SkillPicker(
                    SkillPickerIntent::Step(Direction::Backward),
                )),
                KeyCode::Down if key.modifiers.is_empty() => Routed::Intent(
                    TuiIntent::SkillPicker(SkillPickerIntent::Step(Direction::Forward)),
                ),
                KeyCode::Tab | KeyCode::Enter if key.modifiers.is_empty() => {
                    Routed::Intent(TuiIntent::SkillPicker(SkillPickerIntent::Accept))
                }
                _ => text_key(key),
            };
        }

        // A modal owns every non-global key. In particular, inspector and selection chords must
        // not reach a surface hidden underneath it (SURF-4).
        if context.focused == Some(SurfaceId::Approval) {
            return match key.code {
                KeyCode::Esc => self.escape(context),
                KeyCode::Up | KeyCode::Char('k') => Routed::Intent(TuiIntent::Approval(
                    ApprovalIntent::Move(Direction::Backward),
                )),
                KeyCode::Down | KeyCode::Char('j') => Routed::Intent(TuiIntent::Approval(
                    ApprovalIntent::Move(Direction::Forward),
                )),
                KeyCode::Enter => Routed::Intent(TuiIntent::Approval(ApprovalIntent::Decide)),
                // The same chord that discloses a tool entry, doing the same thing to the request
                // one is asking about. A page key would be a second gesture for one idea, and on
                // a Mac laptop it is a key the keyboard does not have.
                KeyCode::Char('o' | 'O') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Routed::Intent(TuiIntent::Approval(ApprovalIntent::ToggleDetail))
                }
                KeyCode::BackTab => Routed::Intent(TuiIntent::CycleFocus(Direction::Backward)),
                KeyCode::Tab => Routed::Intent(TuiIntent::CycleFocus(Direction::Forward)),
                _ => Routed::Ignored(Ignored::Unbound),
            };
        }

        if context.focused == Some(SurfaceId::Configuration) {
            return match key.code {
                KeyCode::Esc => self.escape(context),
                KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                    step(Direction::Backward, ScrollDirection::Up, context)
                }
                KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                    step(Direction::Forward, ScrollDirection::Down, context)
                }
                _ => Routed::Ignored(Ignored::Unbound),
            };
        }

        if let Some(intent) = inspector_chord(key).or_else(|| selection_chord(key)) {
            return Routed::Intent(intent);
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
        // One intent for both remaining rungs: which of them is innermost is a fact about the
        // projection, and the router owning a second opinion about it is how the two drift.
        if context.selecting || context.dismissible {
            return Routed::Intent(TuiIntent::Dismiss);
        }
        Routed::Ignored(Ignored::NothingToDismiss)
    }

    fn on_mouse(&mut self, mouse: MouseEvent, context: &RouterContext<'_>) -> Routed {
        // No modifier is reserved here. A terminal that offers an escape hatch from mouse reporting
        // takes the gesture before this process sees it — `Shift` in xterm, `Option` in iTerm2 —
        // so an event that *arrives* carrying one is an event that terminal chose to forward, and
        // dropping it made the modifier a dead key on exactly the terminals it was meant to help
        // (INV-8). Every gesture that reaches us reaches a surface, and the selection it makes is
        // semantic rather than a rectangle of cells, which is the stronger half of the promise.
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
            MouseEventKind::Moved => Routed::Intent(TuiIntent::Hover {
                surface: context.surfaces.hit_test(at),
                at,
            }),
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
/// the spatial-memory failure the contract exists to prevent (ui-ux §nested scrolling).
fn scroll(at: Point, direction: ScrollDirection, context: &RouterContext<'_>) -> Routed {
    if context.surfaces.hit_test(at) == Some(SurfaceId::SkillPicker) {
        return Routed::Intent(TuiIntent::SkillPicker(SkillPickerIntent::Step(
            match direction {
                ScrollDirection::Up => Direction::Backward,
                ScrollDirection::Down => Direction::Forward,
            },
        )));
    }
    if context.surfaces.hit_test(at) == Some(SurfaceId::CommandPalette) {
        return Routed::Intent(TuiIntent::CommandPalette(CommandPaletteIntent::Step(
            match direction {
                ScrollDirection::Up => Direction::Backward,
                ScrollDirection::Down => Direction::Forward,
            },
        )));
    }
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
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let commanded = control || alt;
    // Editing chords resolve before the plain-character arm, which is what `commanded` excludes
    // them from: under a cursor a control chord is an edit, never the letter it carries (INV-2).
    if let Some(intent) = editing_chord(key.code, control, alt) {
        return Routed::Intent(TuiIntent::Text(intent));
    }
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

/// The editing grammar over one insertion point, shared by every text input (COM-2).
///
/// Readline's chords rather than a second vocabulary: they are what a terminal user's hands already
/// know, and none of them collides with a chord the workspace has already claimed.
fn editing_chord(code: KeyCode, control: bool, alt: bool) -> Option<TextIntent> {
    let motion = match code {
        KeyCode::Left if control || alt => Motion::WordLeft,
        KeyCode::Right if control || alt => Motion::WordRight,
        KeyCode::Left => Motion::Left,
        KeyCode::Right => Motion::Right,
        KeyCode::Home => Motion::LineStart,
        KeyCode::End => Motion::LineEnd,
        KeyCode::Char('a') if control => Motion::LineStart,
        KeyCode::Char('e') if control => Motion::LineEnd,
        KeyCode::Char('b') if alt => Motion::WordLeft,
        KeyCode::Char('f') if alt => Motion::WordRight,
        KeyCode::Delete => return Some(TextIntent::DeleteForward),
        KeyCode::Char('w') if control => return Some(TextIntent::DeleteWordBackward),
        KeyCode::Char('u') if control => return Some(TextIntent::KillToLineStart),
        KeyCode::Char('k') if control => return Some(TextIntent::KillToLineEnd),
        _ => return None,
    };
    Some(TextIntent::Move(motion))
}

/// Second-window chords, which resolve before keyboard focus is consulted.
///
/// Before the split because the window holds a text input while it is focused, and a control
/// chord is never text (INV-2). Translated whether or not anything is open: the router says what
/// the user pressed, and whether there is something to act on is the reducer's question.
fn inspector_chord(key: KeyEvent) -> Option<TuiIntent> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let intent = match key.code {
        KeyCode::Char('f') if control && !shift => InspectorIntent::ToggleMaximize,
        // Locked in `ui-ux.md` as the keyboard equivalent of dragging the bottom edge (ui-ux §drag
        // scope).
        KeyCode::Down if control && shift => InspectorIntent::Grow,
        KeyCode::Up if control && shift => InspectorIntent::Shrink,
        _ => return None,
    };
    Some(TuiIntent::Inspector(intent))
}

/// Selection chords, which resolve before keyboard focus is consulted.
///
/// Before the split for the same reason the inspector's are: the inspector holds a text input while
/// it is focused, and its artifacts and mail would otherwise be the one content in the workspace
/// that cannot be selected by keyboard. `Shift` with an arrow produces no character, so nothing here
/// can be text.
///
/// Copy is `Ctrl-Y` and not `Ctrl-C`, because `Ctrl-C` is the interrupt that clears a draft (INV-7)
/// and a key that sometimes copies and sometimes discards is worse than an unfamiliar one. The cost
/// is real, and the `Shift` escape hatch to the terminal's own copy is what covers the habit.
fn selection_chord(key: KeyEvent) -> Option<TuiIntent> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let intent = match key.code {
        KeyCode::Down if shift && !control => SelectionIntent::Extend(Direction::Forward),
        KeyCode::Up if shift && !control => SelectionIntent::Extend(Direction::Backward),
        KeyCode::Char('o') if control && !shift => SelectionIntent::ToggleOpen,
        KeyCode::Char('y') if control && !shift => SelectionIntent::Copy,
        _ => return None,
    };
    Some(TuiIntent::Selection(intent))
}

/// Keys addressed to a navigational surface, where no cursor exists.
fn navigation_key(key: KeyEvent, context: &RouterContext<'_>) -> Routed {
    if !key.modifiers.is_empty() {
        return Routed::Ignored(Ignored::Unbound);
    }
    match key.code {
        KeyCode::Char('r') if context.focused == Some(SurfaceId::Transcript) => {
            Routed::Intent(TuiIntent::Retry(crate::RetryAction::Retry))
        }
        KeyCode::Char('e') if context.focused == Some(SurfaceId::Transcript) => {
            Routed::Intent(TuiIntent::Retry(crate::RetryAction::EditRetry))
        }
        // `Enter` means "open what I am on". In the queue that is a request, and going to it is
        // the user choosing to, which is the only way a background request ever moves anything.
        KeyCode::Enter if context.focused == Some(SurfaceId::Attention) => {
            Routed::Intent(TuiIntent::Attention(AttentionIntent::GoTo))
        }
        // Opening is explicit and never a side effect of moving around (INS-4). It does not move
        // the selection: inspection is its own axis, which is what puts two agents on screen.
        KeyCode::Enter => Routed::Intent(TuiIntent::Inspector(InspectorIntent::Open)),
        KeyCode::Down | KeyCode::Char('j') => {
            step(Direction::Forward, ScrollDirection::Down, context)
        }
        KeyCode::Up | KeyCode::Char('k') => step(Direction::Backward, ScrollDirection::Up, context),
        _ => Routed::Ignored(Ignored::Unbound),
    }
}

/// One step down or up, meaning whatever "down" means inside the surface that holds focus.
///
/// The rail is the only navigational surface made of choices, so it is the only one where an arrow
/// moves a selection; everywhere else the content is longer than the region and an arrow is the
/// keyboard equivalent of the wheel, which every gesture must have (`ui-ux.md` §user control).
///
/// Before this, arrows moved the agent selection from any navigational surface. That was defensible
/// with one list on screen and stops being so with two, and it left the wheel as the workspace's
/// only interaction with no keyboard equivalent.
fn step(list: Direction, wheel: ScrollDirection, context: &RouterContext<'_>) -> Routed {
    match context.focused {
        Some(SurfaceId::Agents) => Routed::Intent(TuiIntent::MoveSelection(list)),
        Some(SurfaceId::Attention) => {
            Routed::Intent(TuiIntent::Attention(AttentionIntent::Move(list)))
        }
        Some(surface) => {
            if context
                .surfaces
                .viewport(surface)
                .is_some_and(Viewport::is_scrollable)
            {
                Routed::Intent(TuiIntent::Scroll {
                    surface,
                    direction: wheel,
                })
            } else {
                Routed::Ignored(Ignored::NothingScrollable)
            }
        }
        None => Routed::Ignored(Ignored::Unbound),
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
        intent::{
            ApprovalIntent, Direction, InspectorIntent, PointerIntent, ScrollDirection,
            SelectionIntent, TextIntent, TuiIntent,
        },
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
                    content_width: bounds.width,
                    visible_rows: bounds.height,
                    offset: 0,
                }),
            })
            .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
        }
        tree
    }

    /// Focus on the agent rail, which is where an arrow means "another agent".
    fn context(
        surfaces: &SurfaceTree,
        focus: KeyboardFocus,
        dismissible: bool,
    ) -> RouterContext<'_> {
        focused_on(SurfaceId::Agents, surfaces, focus, dismissible)
    }

    fn focused_on(
        focused: SurfaceId,
        surfaces: &SurfaceTree,
        focus: KeyboardFocus,
        dismissible: bool,
    ) -> RouterContext<'_> {
        RouterContext {
            surfaces,
            focus,
            focused: Some(focused),
            dismissible,
            selecting: false,
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

    /// INV-10: an arrow moves within whatever holds focus, and the wheel finally has a keyboard
    /// equivalent.
    #[test]
    fn an_arrow_moves_the_rail_and_scrolls_everything_else() {
        let surfaces = tree();
        let mut router = Router::default();
        let down = key(KeyCode::Down, KeyModifiers::NONE);

        assert_eq!(
            router.translate(&down, &context(&surfaces, KeyboardFocus::Navigation, false)),
            Routed::Intent(TuiIntent::MoveSelection(Direction::Forward)),
            "the rail is the one navigational surface made of choices"
        );
        assert_eq!(
            router.translate(
                &down,
                &focused_on(PANEL, &surfaces, KeyboardFocus::Navigation, false)
            ),
            Routed::Intent(TuiIntent::Scroll {
                surface: PANEL,
                direction: ScrollDirection::Down,
            }),
            "elsewhere an arrow is the wheel, addressed to where the user is"
        );
        assert_eq!(
            router.translate(
                &key(KeyCode::Char('k'), KeyModifiers::NONE),
                &focused_on(OVERLAY, &surfaces, KeyboardFocus::Navigation, false)
            ),
            Routed::Intent(TuiIntent::Scroll {
                surface: OVERLAY,
                direction: ScrollDirection::Up,
            }),
            "the vim keys mean exactly what the arrows mean, or they are a second grammar"
        );
        assert_eq!(
            router.translate(
                &down,
                // Registered by no frame, so it has no viewport and nowhere to go.
                &focused_on(
                    SurfaceId::Composer,
                    &surfaces,
                    KeyboardFocus::Navigation,
                    false
                )
            ),
            Routed::Ignored(Ignored::NothingScrollable),
            "a surface with nowhere to scroll declines by name rather than moving the rail"
        );
    }

    /// SURF-4 and INV-10: modal navigation is a closed grammar, including a keyboard equivalent
    /// for scrolling long decision detail.
    #[test]
    fn approval_keys_stay_inside_the_blocking_surface() {
        let mut surfaces = tree();
        surfaces
            .insert(Surface {
                id: SurfaceId::Approval,
                bounds: Rect::new(4, 2, 30, 12),
                z_index: 10,
                kind: SurfaceKind::Modal,
                viewport: Some(Viewport {
                    content_rows: 40,
                    content_width: 28,
                    visible_rows: 10,
                    offset: 0,
                }),
            })
            .unwrap_or_else(|error| panic!("fixture must insert: {error}"));
        let context = focused_on(
            SurfaceId::Approval,
            &surfaces,
            KeyboardFocus::Navigation,
            true,
        );
        let mut router = Router::default();

        assert_eq!(
            router.translate(&key(KeyCode::Up, KeyModifiers::NONE), &context),
            Routed::Intent(TuiIntent::Approval(ApprovalIntent::Move(
                Direction::Backward
            )))
        );
        assert_eq!(
            router.translate(&key(KeyCode::Enter, KeyModifiers::NONE), &context),
            Routed::Intent(TuiIntent::Approval(ApprovalIntent::Decide))
        );
        assert_eq!(
            router.translate(&key(KeyCode::Char('o'), KeyModifiers::CONTROL), &context),
            Routed::Intent(TuiIntent::Approval(ApprovalIntent::ToggleDetail)),
            "the disclosure chord discloses the request rather than scrolling the region"
        );
        assert_eq!(
            router.translate(&key(KeyCode::PageDown, KeyModifiers::NONE), &context),
            Routed::Ignored(Ignored::Unbound),
            "the decision region does not scroll: its options are its last rows at every size"
        );
        assert_eq!(
            router.translate(&key(KeyCode::Char('f'), KeyModifiers::CONTROL), &context),
            Routed::Ignored(Ignored::Unbound),
            "an inspector chord cannot reach the workspace under a modal"
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

    /// INV-3: bare motion names only the topmost surface under the pointer and names `None` when it
    /// leaves the workspace, so the reducer can clear a previous visual hover without guessing.
    #[test]
    fn pointer_motion_routes_a_hover_without_capture_or_focus() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();

        assert_eq!(
            router.translate(&mouse(MouseEventKind::Moved, 12, 4), &context),
            Routed::Intent(TuiIntent::Hover {
                surface: Some(OVERLAY),
                at: Point { x: 12, y: 4 },
            })
        );
        assert_eq!(
            router.translate(&mouse(MouseEventKind::Moved, 99, 99), &context),
            Routed::Intent(TuiIntent::Hover {
                surface: None,
                at: Point { x: 99, y: 99 },
            })
        );
        assert_eq!(router.capture(), None);
    }

    /// ENT-4: disclosure is a global selection chord, including while a text input owns the
    /// cursor; the reducer decides whether the selection's moving end is foldable.
    #[test]
    fn ctrl_o_is_the_same_disclosure_intent_under_both_focus_modes() {
        let surfaces = tree();
        let mut router = Router::default();
        let chord = key(KeyCode::Char('o'), KeyModifiers::CONTROL);

        for focus in [KeyboardFocus::Navigation, KeyboardFocus::TextInput] {
            assert_eq!(
                router.translate(&chord, &context(&surfaces, focus, false)),
                Routed::Intent(TuiIntent::Selection(SelectionIntent::ToggleOpen))
            );
        }
    }

    /// INV-3: eligibility is whether a viewport can move, not what is on top.
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
                ..Viewport::default()
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
                ..Viewport::default()
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
                    ..Viewport::default()
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

    /// INV-4: losing terminal focus pauses observation without discarding the gesture's owner.
    #[test]
    fn focus_loss_suspends_motion_without_releasing_capture() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);
        let mut router = Router::default();
        router.translate(&press(12, 4), &context);

        assert_eq!(
            router.translate(&Event::FocusLost, &context),
            Routed::Intent(TuiIntent::Pointer(PointerIntent::Suspend {
                surface: OVERLAY
            }))
        );
        assert_eq!(router.capture(), Some(OVERLAY));
        assert_eq!(
            router.translate(
                &mouse(MouseEventKind::Drag(MouseButton::Left), 12, 6),
                &context
            ),
            Routed::Intent(TuiIntent::Pointer(PointerIntent::Drag {
                surface: OVERLAY,
                at: Point { x: 12, y: 6 }
            }))
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

    /// The inspector grammar, tested as one grammar rather than key by key.
    ///
    /// Every chord resolves the same way under both focus modes, which is what makes them reachable
    /// while the inspector's own input holds the cursor. `Enter` is the deliberate exception and the
    /// reason the chords are chords: under text focus it submits, so opening cannot live there.
    #[test]
    fn the_inspector_grammar_is_the_same_under_both_focus_modes_except_enter() {
        let surfaces = tree();
        let mut router = Router::default();
        let chords = [
            (
                key(KeyCode::Char('f'), KeyModifiers::CONTROL),
                InspectorIntent::ToggleMaximize,
            ),
            (
                key(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
                InspectorIntent::Grow,
            ),
            (
                key(KeyCode::Up, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
                InspectorIntent::Shrink,
            ),
        ];

        for focus in [KeyboardFocus::Navigation, KeyboardFocus::TextInput] {
            let context = context(&surfaces, focus, true);
            for (event, expected) in &chords {
                assert_eq!(
                    router.translate(event, &context),
                    Routed::Intent(TuiIntent::Inspector(*expected)),
                    "{event:?} must mean the same thing under {focus:?}"
                );
            }
        }

        let navigation = context(&surfaces, KeyboardFocus::Navigation, false);
        let typing = context(&surfaces, KeyboardFocus::TextInput, false);
        let enter = key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            router.translate(&enter, &navigation),
            Routed::Intent(TuiIntent::Inspector(InspectorIntent::Open)),
            "opening is explicit, and there is no cursor to submit to"
        );
        assert_eq!(
            router.translate(&enter, &typing),
            Routed::Intent(TuiIntent::Text(TextIntent::Submit)),
            "the same key submits where a cursor exists (INV-2)"
        );
        // Plain arrows keep moving the selection; only the chord reaches the inspector.
        assert_eq!(
            router.translate(&key(KeyCode::Down, KeyModifiers::NONE), &navigation),
            Routed::Intent(TuiIntent::MoveSelection(Direction::Forward))
        );
    }

    /// INV-7: quitting is deliberate, and a bare letter is never it.
    ///
    /// `q` used to quit under navigation focus. Focus starts on a navigation surface and moves
    /// without the screen saying so, so the first letter of a message typed one `Tab` too early
    /// ended the session. A chord is the only shape a quit key can have, and `Ctrl-C`, the chord
    /// every hand already knows, is the interrupt rather than the exit under both focuses.
    #[test]
    fn quit_is_explicit_and_unreachable_while_typing() {
        let surfaces = tree();
        let mut router = Router::default();

        for focus in [KeyboardFocus::TextInput, KeyboardFocus::Navigation] {
            assert_eq!(
                router.translate(
                    &key(KeyCode::Char('d'), KeyModifiers::CONTROL),
                    &context(&surfaces, focus, true)
                ),
                Routed::Intent(TuiIntent::Quit)
            );
            assert_eq!(
                router.translate(
                    &key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                    &context(&surfaces, focus, true)
                ),
                Routed::Intent(TuiIntent::Interrupt),
                "Ctrl-C is the interrupt, never the exit"
            );
        }
        assert_eq!(
            router.translate(
                &key(KeyCode::Char('q'), KeyModifiers::NONE),
                &context(&surfaces, KeyboardFocus::TextInput, false)
            ),
            Routed::Intent(TuiIntent::Text(TextIntent::Insert('q')))
        );
        for dismissible in [true, false] {
            assert_eq!(
                router.translate(
                    &key(KeyCode::Char('q'), KeyModifiers::NONE),
                    &context(&surfaces, KeyboardFocus::Navigation, dismissible)
                ),
                Routed::Ignored(Ignored::Unbound),
                "a bare letter is not a quit key, whatever is open"
            );
        }
    }

    /// INV-8: a forwarded gesture is a gesture for this workspace, whatever it carries.
    ///
    /// A terminal that bypasses mouse reporting keeps the event; one that forwards it has decided
    /// this process should have it. Dropping it on a modifier left the user with a chord that
    /// selected nothing on the terminals that forward, and had no effect on the ones that do not.
    #[test]
    fn a_modifier_does_not_make_a_pointer_event_disappear() {
        let surfaces = tree();
        let context = context(&surfaces, KeyboardFocus::Navigation, false);

        for modifiers in [KeyModifiers::SHIFT, KeyModifiers::ALT, KeyModifiers::NONE] {
            let mut router = Router::default();
            let event = Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 12,
                row: 4,
                modifiers,
            });
            assert_eq!(
                router.translate(&event, &context),
                Routed::Intent(TuiIntent::Pointer(PointerIntent::Press {
                    surface: OVERLAY,
                    at: Point { x: 12, y: 4 },
                })),
                "{modifiers:?} reaches the same surface a bare press does"
            );
            assert_eq!(router.capture(), Some(OVERLAY));
        }
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
