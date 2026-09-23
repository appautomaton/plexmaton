//! Phase 00 view state, interaction surfaces, and Ratatui projection.
//!
//! The entry point is [`Workspace`]: events in, terminal events in, at most one frame out. Laying
//! the workspace out is deliberately not part of this surface. A caller who could compute geometry
//! without drawing it could hand routing rectangles no frame ever painted, and "an event resolves
//! against the frame that was drawn" is worth more as a property of the API than as a rule people
//! remember ([`specs/frame-loop.md`](../../../.agents/specs/frame-loop.md) FR-3).

mod content;
mod content_permissions;
#[cfg(test)]
mod frames;
mod intent;
#[cfg(test)]
mod journey;
mod layout;
mod markdown;
pub mod preparation;
mod render;
mod router;
mod state;
mod statusline;
mod surface;
#[cfg(test)]
mod test_support;
mod text_layout;
mod theme;
mod transcript;
mod workspace;

pub use intent::{
    ApprovalIntent, Direction, InspectorIntent, MenuIntent, PointerIntent, ScrollDirection,
    SelectionIntent, TextIntent, TreeIntent, TuiIntent,
};
pub use render::render;
pub use router::{Ignored, Routed, Router, RouterContext};
pub use state::{
    AgentView, ApplyOutcome, ApprovalSubmission, ApprovalView, ArtifactView, AttentionView,
    ChildControl, ChildControlRefusal, ChildControlSnapshot, CleanupNotice, Command, CommandFlag,
    CommandFlags, CompactRefusal, CompactionNote, ConfigurationSummary, ConversationChoice,
    ConversationPickerStatus, ConversationRequest, ConversationRestoration, ConversationTailRepair,
    CopyReceipt, CopyRequest, Drawer, HandoffView, InspectorView, Listing,
    MAX_CONVERSATION_CHOICES, MailView, ModelChoice, ModelIdentity, NoticeView, Page,
    PermissionRequest, PersistenceNotice, QueuedBoundary, QueuedInput, ReduceError, RetryAction,
    RetryActions, RetrySubmission, RetryTarget, ScrollPosition, Selection, ServerToolView,
    SkillChoice, SkillChoiceSource, Submission, SubmissionKind, SwitchRefusal, TaskView,
    ToolCallView, TranscriptEntryView, TranscriptItemView, TranscriptTextKind, ViewRevision,
    ViewState,
};
pub use state::{ApprovalChoice, ApprovalFeedback, ApprovalStage};
pub use statusline::{StatusLineText, StatusLineTextError};
pub use surface::{
    KeyboardFocus, Point, Surface, SurfaceId, SurfaceKind, SurfaceTree, SurfaceTreeError, Viewport,
};
pub use theme::{
    EFFORT_COLOR_PHASES, EffortPalette, MarkdownTheme, Palette, Role, Slots, agent_role, invert,
    tool_role,
};
pub use transcript::TranscriptMetrics;
pub use workspace::{
    CommandRun, CommandTarget, EffortChange, Flow, FrameWork, ModelChange, Outcome, TreeRequest,
    Workspace,
};
pub mod math;

/// The mark: its block for the terminal's cell, the greeting's timeline, and its rows at a moment.
pub mod mark {
    pub use crate::render::mark::{CellSize, Moment, greeting, lines, size};
}
