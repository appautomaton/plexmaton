//! What crosses the loop's edge: what it is told, and what it asks for in return.
//!
//! Both directions are values. Nothing here can be performed by this crate, which is what keeps
//! the decision and the doing in different places — and what lets a whole turn be driven from a
//! script with no network, no clock and no terminal.

use plexmaton_core::{SessionEventEnvelope, ToolCallId};

use crate::model::{ModelError, ModelEvent, ModelRequest};
use crate::tools::{ToolCall, ToolOutcome};

/// Something the loop is told.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input {
    /// The user submitted a message to this agent.
    Submitted {
        /// Exact text the user submitted.
        text: String,
    },
    /// The model produced something.
    Streamed(ModelEvent),
    /// The step failed before it could finish.
    Failed(ModelError),
    /// A dispatched tool call ended.
    ToolFinished {
        /// The call being answered.
        call_id: ToolCallId,
        /// How it ended.
        outcome: ToolOutcome,
    },
    /// The user asked the current turn to stop.
    Interrupted,
}

/// Something the loop needs performed, and cannot perform itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Ask the model, and feed what it says back in as [`Input::Streamed`].
    CallModel(ModelRequest),
    /// Run one call, and feed the result back in as [`Input::ToolFinished`].
    RunTool(ToolCall),
}

/// What one input produced.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Reaction {
    /// Events for the projection, numbered on this agent's one sequence.
    pub events: Vec<SessionEventEnvelope>,
    /// Work for whoever owns the outside world.
    pub effects: Vec<Effect>,
}
