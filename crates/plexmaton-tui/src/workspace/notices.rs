//! What the composition root tells the user without touching the transcript.
//!
//! A notice is the workspace's own voice: it reports something the root observed that no producer
//! event will say, and it costs the conversation nothing. These are the routes into it, kept
//! together because every one of them is the same shape — a bounded sentence, no semantic fact.

use super::Workspace;

impl Workspace {
    /// Shows one bounded skill discovery, load, or activation diagnostic.
    pub fn report_skill_diagnostic(&mut self, message: String) {
        self.state.report_skill_diagnostic(message);
    }

    /// Reports input the runtime refused, beside the draft it was returned to (COM-3).
    ///
    /// The refusal reaches the user here rather than through the process exit it used to cause: a
    /// request the runtime would not take is worth one sentence, never the conversation on screen.
    pub fn report_dispatch_refusal(&mut self, message: String) {
        self.state.report_dispatch_refusal(message);
    }

    /// Reports that the model just selected reads earlier replies as text (MDL-1).
    ///
    /// The switch already succeeded, so this is a receipt and not a question: nothing was
    /// destroyed, and selecting the original model again replays its own history exactly.
    pub fn report_degraded_history(&mut self) {
        self.state.report_degraded_history();
    }
}
