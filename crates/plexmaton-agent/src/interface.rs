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
    /// The user submitted a message for this agent's next turn.
    Submitted {
        /// Exact text the user submitted.
        text: String,
    },
    /// The user amended the turn in flight, for its next step.
    Steered {
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

/// Why user input could not be claimed by the boundary it named.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UndeliveredReason {
    /// Steering named a current turn, but no turn was open.
    NoActiveTurn,
    /// The current turn ended without opening another step.
    TurnEnded,
    /// The user stopped the turn before its pending input was claimed.
    Interrupted,
    /// The step failed before its pending input was claimed.
    StepFailed,
    /// The turn spent its step budget before it could open another one.
    StepBudgetReached,
    /// The bounded input queue had no room for another entry.
    QueueFull,
}

/// User input the loop did not deliver, retaining both its payload and the reason (LOOP-6).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndeliveredInput {
    /// Exact text the user submitted.
    pub text: String,
    /// Why the boundary could not claim it.
    pub reason: UndeliveredReason,
}

impl UndeliveredInput {
    pub(crate) const fn new(text: String, reason: UndeliveredReason) -> Self {
        Self { text, reason }
    }
}

/// What one input produced.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Reaction {
    /// Events for the projection, numbered on this agent's one sequence.
    pub events: Vec<SessionEventEnvelope>,
    /// Work for whoever owns the outside world.
    pub effects: Vec<Effect>,
    /// User input whose intended boundary cannot claim it. Ownership returns to the caller with
    /// the exact payload instead of leaving it in a queue that a later turn could misread.
    pub undelivered: Vec<UndeliveredInput>,
}
