//! Phase 00 view state, interaction surfaces, and Ratatui projection.

mod render;
mod state;
mod surface;

pub use render::{LayoutClass, render};
pub use state::{
    AgentView, ApplyOutcome, ArtifactView, AttentionView, MailView, NoticeView, ReduceError,
    ToolActivityView, TranscriptItemView, ViewRevision, ViewState,
};
pub use surface::{Point, Surface, SurfaceId, SurfaceTree, SurfaceTreeError};
