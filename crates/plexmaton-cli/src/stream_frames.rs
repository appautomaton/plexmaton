//! Bounded presentation batching, not a second transcript (FR-5).
//!
//! The runtime already owns these semantic events. Holding only pending deltas here lets input
//! resolve against the text and geometry actually painted, then applies every revision in order.

use std::time::{Duration, Instant};

use plexmaton_core::{SessionEvent, SessionEventEnvelope};
use plexmaton_tui::{FrameWork, Outcome, Workspace};
use ratatui::{Terminal, backend::Backend, crossterm::event::Event};

pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const MAX_EVENTS: usize = 64;
const MAX_TEXT_BYTES: usize = 128 * 1024;

/// Only stream presentation waits; input, lifecycle changes and resource pressure may paint sooner.
pub(crate) struct StreamFrames {
    pending: Vec<SessionEventEnvelope>,
    text_bytes: usize,
    not_before: Instant,
}

impl StreamFrames {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            pending: Vec::new(),
            text_bytes: 0,
            not_before: now,
        }
    }

    /// Retain only small text deltas. A non-text transition cannot overtake pending revisions.
    pub(crate) fn receive(&mut self, workspace: &mut Workspace, event: SessionEventEnvelope) {
        let SessionEvent::TranscriptDelta { text, .. } = &event.event else {
            self.flush(workspace);
            workspace.emit(vec![event]);
            return;
        };
        // Count allocation capacity, not visible length: a short String can retain a large buffer.
        let bytes = text.capacity();
        if bytes > MAX_TEXT_BYTES {
            self.flush(workspace);
            workspace.emit(vec![event]);
            return;
        }
        if self.text_bytes + bytes > MAX_TEXT_BYTES {
            self.flush(workspace);
        }
        self.text_bytes += bytes;
        self.pending.push(event);
        if self.pending.len() == MAX_EVENTS || self.text_bytes == MAX_TEXT_BYTES {
            self.flush(workspace);
        }
    }

    /// No pending presentation means no timer; more deltas never push this deadline back.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        (!self.pending.is_empty()).then_some(self.not_before)
    }

    /// Commit presentation before a runtime report, replacement or final completion is projected.
    pub(crate) fn flush(&mut self, workspace: &mut Workspace) {
        if !self.pending.is_empty() {
            self.text_bytes = 0;
            workspace.emit(std::mem::take(&mut self.pending));
        }
    }

    /// Hit testing and copy must run before queued text can change the drawn Markdown mapping.
    pub(crate) fn handle(&mut self, workspace: &mut Workspace, event: &Event) -> Outcome {
        let outcome = workspace.handle(event);
        self.flush(workspace);
        outcome
    }

    /// The production and measurement path. A failed draw never advances the frame deadline.
    #[allow(
        dead_code,
        reason = "the cell-only wrapper is used by the separate CPU measurement binary and fixtures"
    )]
    pub(crate) fn draw<B: Backend>(
        &mut self,
        workspace: &mut Workspace,
        terminal: &mut Terminal<B>,
        now: Instant,
    ) -> Result<Option<FrameWork>, B::Error> {
        self.draw_with_native(workspace, terminal, now, |_, _| Ok(()))
    }

    pub(crate) fn draw_with_native<B: Backend>(
        &mut self,
        workspace: &mut Workspace,
        terminal: &mut Terminal<B>,
        now: Instant,
        output: impl FnMut(&mut B, plexmaton_tui::math::NativeStage<'_>) -> Result<(), B::Error>,
    ) -> Result<Option<FrameWork>, B::Error> {
        if workspace.needs_draw() || now >= self.not_before {
            self.flush(workspace);
        }
        let work = workspace.draw_with_native(terminal, output)?;
        if work.is_some() {
            self.not_before = now + FRAME_INTERVAL;
        }
        Ok(work)
    }
}

#[cfg(test)]
#[path = "stream_frames/tests.rs"]
mod tests;
