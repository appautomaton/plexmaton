//! The workspace's typed input vocabulary.
//!
//! An intent says what the user asked for, never what should change. Keeping the two apart is what
//! lets one router own terminal-event translation while the reducer stays testable without a
//! terminal. The numbered invariants live in `.agents/specs/interaction-routing.md`.
//!
//! Rejected: putting this vocabulary in `plexmaton-core`. Scroll, focus and pointer capture are no
//! runtime's business; a user action that must reach one becomes a core command at the
//! composition boundary.

use crate::{
    state::Motion,
    surface::{Point, SurfaceId},
};

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

/// One operation on the primary composer's inline skill completion list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkillPickerIntent {
    Step(Direction),
    Accept,
    Close,
}

/// An edit addressed to whichever text input currently holds the cursor.
///
/// The intent never names its target: exactly one cursor exists, so the target is a fact about
/// focus rather than something a producer could get wrong.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextIntent {
    /// Insert one character at the cursor.
    Insert(char),
    /// Insert a terminal paste as text, without interpreting its newlines as submission.
    Paste(String),
    /// Remove the grapheme before the cursor.
    DeleteBackward,
    /// Remove the grapheme at the cursor, which does not move.
    DeleteForward,
    /// Remove the whitespace and then the word before the cursor.
    DeleteWordBackward,
    /// Remove everything between the start of the logical line and the cursor.
    KillToLineStart,
    /// Remove everything between the cursor and the end of the logical line.
    KillToLineEnd,
    /// Move the cursor without changing the text.
    Move(Motion),
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
    /// Pointer observation paused while the terminal lost focus; capture remains held.
    Suspend {
        /// Surface whose in-flight gesture may resume with the next drag.
        surface: SurfaceId,
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

/// One thing the user asked of the second window.
///
/// Nothing here opens it or chooses the agent it shows: the window is the selection (INS-1), so
/// `MoveSelection` is what puts a second agent on screen. These verbs act on the window the
/// selection already opened: entering it, maximizing it, and moving its edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorIntent {
    /// Enter the second window, so its input takes the keyboard. Opening it is selecting an agent
    /// other than the primary (INS-1), which is not a command to the window at all.
    Open,
    /// Toggle the full-region presentation.
    ToggleMaximize,
    /// Take one more row from the conversation, within its guarantee.
    Grow,
    /// Give one row back.
    Shrink,
}

/// One thing the user asked of the Attention queue.
///
/// Every verb here is user-initiated. Nothing a background agent does reaches this enum, which is
/// the structural half of "a request never takes focus": there is no producer path to these.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttentionIntent {
    /// Move the queue's own cursor.
    Move(Direction),
    /// Go to the agent whose request is under the cursor, marking it seen.
    GoTo,
}

/// One thing the user asked of an approval surface they explicitly opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalIntent {
    /// Move between the current producer-supported choices.
    Move(Direction),
    /// Return the highlighted typed decision to the owning loop.
    Decide,
    /// Show the request's detail in full, or clip it back to one row.
    ToggleDetail,
}

/// One thing the user asked of the selection.
///
/// There is no `Clear` here: clearing arrives as `Dismiss`, because the `Escape` ladder resolves
/// exactly one layer per press and a selection is one of its rungs (INV-6).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionIntent {
    /// Extend the selection one entry, starting one if there is none.
    Extend(Direction),
    /// Toggle disclosure for the entry at the moving end of the selection.
    ToggleOpen,
    /// Put the selected content on the clipboard.
    Copy,
}

/// One thing the user asked the workspace to do.
///
/// This is deliberately not a universal application event: semantic runtime transitions arrive as
/// `plexmaton_core::ConversationEvent`, and the two vocabularies never merge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TuiIntent {
    /// Navigate, accept, or close the primary composer's skill completions.
    SkillPicker(SkillPickerIntent),
    /// Contextual action on the primary conversation's eligible failed message.
    Retry(crate::RetryAction),
    /// Pull the Drawer open, move through it, or choose the row under the marker.
    Drawer(DrawerIntent),
    /// The quit chord, `Ctrl-D`. The reducer asks on the first press and leaves only when a second
    /// arrives inside its one-second window, so a quit is never one keystroke (INV-7).
    Quit,
    /// `Ctrl-C`, the shell's interrupt: clears the resolved conversation's draft, or if it is
    /// empty, addresses that conversation's running turn. Never both, and never a quit.
    Interrupt,
    /// Move keyboard focus one stop around the focus ring.
    CycleFocus(Direction),
    /// Move the agent selection one step, without opening anything.
    MoveSelection(Direction),
    /// Resolve the topmost dismissible layer.
    Dismiss,
    /// Act on the inspector.
    Inspector(InspectorIntent),
    /// Act on the Attention queue.
    Attention(AttentionIntent),
    /// Act on the open approval surface.
    Approval(ApprovalIntent),
    /// Act on the selection.
    Selection(SelectionIntent),
    /// Scroll the viewport under the pointer. Hover routing never changes focus.
    Scroll {
        /// Surface resolved from the pointer position.
        surface: SurfaceId,
        /// Wheel direction.
        direction: ScrollDirection,
    },
    /// Point at one cell without changing focus or taking capture.
    Hover {
        /// Topmost surface under the pointer, or none outside the workspace.
        surface: Option<SurfaceId>,
        /// Terminal-cell position of the pointer.
        at: Point,
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

/// What the user asked of the Drawer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrawerIntent {
    /// Pull it open on its page list, with an empty filter and the first page chosen.
    Open,
    /// Move the choice by one, stopping at the ends; on a page that scrolls, move the viewport.
    Step(Direction),
    /// Act on the chosen row: open a page, a conversation, or a permission choice.
    Choose,
}
