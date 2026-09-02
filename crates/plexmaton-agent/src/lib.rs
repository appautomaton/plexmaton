//! The agent loop: what a turn does next, decided without doing any of it.
//!
//! This crate holds the machine and nothing else. It has no asynchronous runtime, no HTTP client,
//! no filesystem access and no terminal, and its manifest is where that is enforced: an effect is
//! a value returned to a caller who owns the outside world. That is what makes a turn testable
//! from a script, inspectable between any two inputs, and — when Phase 02 arrives — resumable
//! from the inputs that produced it.
//!
//! The vocabulary crossing outward is [`plexmaton_core::SessionEvent`]; the vocabulary crossing
//! inward is [`Input`]. They never merge.

mod interface;
mod model;
mod record;
mod step;
mod tools;
mod turn;

pub use interface::{Effect, Input, Reaction};
pub use model::{ModelError, ModelEvent, ModelRequest, RequestItem, StopReason};
pub use tools::{ToolCall, ToolOutcome};
pub use turn::{Agent, TurnBudget};
