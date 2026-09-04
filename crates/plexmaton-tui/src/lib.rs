//! Phase 00 view state, interaction surfaces, and Ratatui projection.
//!
//! The entry point is [`Workspace`]: events in, terminal events in, at most one frame out. Laying
//! the workspace out is deliberately not part of this surface. A caller who could compute geometry
//! without drawing it could hand routing rectangles no frame ever painted, and "an event resolves
//! against the frame that was drawn" is worth more as a property of the API than as a rule people
//! remember ([`specs/frame-loop.md`](../../../.agents/specs/frame-loop.md) FR-3).

mod content;
#[cfg(test)]
mod frames;
mod intent;
#[cfg(test)]
mod journey;
mod layout;
mod render;
mod router;
mod state;
mod surface;
#[cfg(test)]
mod test_support;
mod theme;
mod transcript;
mod workspace;

pub use intent::{
    ApprovalIntent, AttentionIntent, Direction, InspectorIntent, PointerIntent, ScrollDirection,
    SelectionIntent, TextIntent, TuiIntent,
};
pub use render::render;
pub use router::{Ignored, Routed, Router, RouterContext};
pub use state::{
    AgentView, ApplyOutcome, ApprovalSubmission, ApprovalView, ArtifactView, AttentionView,
    CleanupNotice, Composer, CopyRequest, InspectorView, MailView, NoticeView, PersistenceNotice,
    ReduceError, ScrollPosition, Selection, SessionRecoveryNotice, Submission, SubmissionKind,
    TailRecoveryNotice, ToolCallView, TranscriptEntryView, TranscriptItemView, TranscriptTextKind,
    ViewRevision, ViewState,
};
pub use surface::{
    KeyboardFocus, Point, Surface, SurfaceId, SurfaceKind, SurfaceTree, SurfaceTreeError, Viewport,
};
pub use theme::{Palette, Role, agent_role, tool_role};
pub use transcript::TranscriptMetrics;
pub use workspace::{Flow, FrameWork, Outcome, Workspace};
