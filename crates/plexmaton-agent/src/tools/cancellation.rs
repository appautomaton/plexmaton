use serde::{Deserialize, Serialize};

/// Why a call was cancelled before producing an ordinary result.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCancellationReason {
    /// The user interrupted the turn.
    Interrupted,
    /// The model step failed while calls were outstanding.
    StepFailed,
    /// The runtime began an orderly shutdown.
    Shutdown,
    /// The prior owner disappeared before it could report whether the call completed.
    ProcessDied,
}
