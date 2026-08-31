//! The workspace's typed input vocabulary.
//!
//! An intent says what the user asked for, never what should change. Keeping the two apart is what
//! lets one router own terminal-event translation while the reducer stays testable without a
//! terminal. The numbered invariants live in `.agents/specs/interaction-routing.md`.

use crate::surface::{Point, SurfaceId};

/// Ordering step shared by focus cycling and list selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// Toward the end of the ring or list.
    Forward,
    /// Toward its beginning.
    Backward,
}

/// Which way a wheel moved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollDirection {
    /// Toward older content.
    Up,
    /// Toward newer content.
    Down,
}

/// An edit addressed to whichever text input currently holds the cursor.
///
/// The intent never names its target: exactly one cursor exists, so the target is a fact about
/// focus rather than something a producer could get wrong.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextIntent {
    /// Insert one character at the cursor.
    Insert(char),
    /// Remove the grapheme before the cursor.
    DeleteBackward,
    /// Break the line without submitting.
    Newline,
    /// Submit the input's current contents.
    Submit,
}

/// One step of a pointer gesture, already resolved to a surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerIntent {
    /// The primary button went down over `surface`, which now holds capture.
    Press {
        /// Surface that was hit.
        surface: SurfaceId,
        /// Terminal-cell position of the press.
        at: Point,
    },
    /// The pointer moved while captured. `at` may lie outside `surface`.
    Drag {
        /// Surface holding capture.
        surface: SurfaceId,
        /// Current terminal-cell position.
        at: Point,
    },
    /// The primary button was released, ending capture.
    Release {
        /// Surface that held capture.
        surface: SurfaceId,
        /// Terminal-cell position of the release.
        at: Point,
    },
    /// The gesture was abandoned rather than completed, so its effect must be undone.
    Cancel {
        /// Surface that held capture.
        surface: SurfaceId,
    },
}

/// One thing the user asked the workspace to do.
///
/// This is deliberately not a universal application event: semantic runtime transitions arrive as
/// `plexmaton_core::PrototypeEvent`, and the two vocabularies never merge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TuiIntent {
    /// Leave the workspace.
    Quit,
    /// Move keyboard focus one stop around the focus ring.
    CycleFocus(Direction),
    /// Move the agent selection one step, without opening anything.
    MoveSelection(Direction),
    /// Resolve the topmost dismissible layer.
    Dismiss,
    /// Scroll the viewport under the pointer. Hover routing never changes focus.
    Scroll {
        /// Surface resolved from the pointer position.
        surface: SurfaceId,
        /// Wheel direction.
        direction: ScrollDirection,
    },
    /// One step of a pointer gesture.
    Pointer(PointerIntent),
    /// One edit addressed to the focused text input.
    Text(TextIntent),
    /// The terminal changed size, in cells.
    TerminalResized {
        /// New width.
        width: u16,
        /// New height.
        height: u16,
    },
}
