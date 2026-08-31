//! Phase 00 view state, interaction surfaces, and Ratatui projection.

mod intent;
mod render;
mod router;
mod state;
mod surface;
mod theme;

pub use intent::{Direction, PointerIntent, ScrollDirection, TextIntent, TuiIntent};
pub use render::{LayoutClass, render};
pub use router::{Ignored, KeyboardFocus, Routed, Router, RouterContext};
pub use state::{
    AgentView, ApplyOutcome, ArtifactView, AttentionView, MailView, NoticeView, ReduceError,
    ToolActivityView, TranscriptItemView, ViewRevision, ViewState,
};
pub use surface::{Point, Surface, SurfaceId, SurfaceTree, SurfaceTreeError};
pub use theme::{Palette, Role, agent_role, tool_role};
