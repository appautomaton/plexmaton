//! PRE-3/MTH-5: publish one source hit map only after the complete cell/native frame succeeds.

use ratatui::{Terminal, backend::Backend};

use super::{FrameWork, Workspace};
use crate::{render::render, surface::SurfaceTree};

impl Workspace {
    /// Draws a frame if the projection changed since the last one, and reports what it cost.
    ///
    /// `None` means nothing needed painting. Ambient background activity and input the workspace
    /// ignores must not cost a full-screen redraw (FR-1), and a caller that cannot tell the
    /// difference cannot measure how often that gate actually fires.
    pub fn draw<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<Option<FrameWork>, B::Error> {
        self.draw_with_native(terminal, |_, _| Ok(()))
    }

    /// Paint cells and native text through one output owner. Only successful completion of both
    /// phases publishes source hit maps; cell-only fixtures may use `draw` to inspect geometry.
    pub fn draw_with_native<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        mut output: impl FnMut(&mut B, crate::math::NativeStage<'_>) -> Result<(), B::Error>,
    ) -> Result<Option<FrameWork>, B::Error> {
        if !self.needs_draw() {
            return Ok(None);
        }
        // Input can select the old painted source after new preparation is admitted. Validate
        // again before painting its replacement, so reinterpreted Markdown cannot keep an old
        // highlight or a ready copy attached to different visible text (FR-3/SEL-1).
        self.validate_text_selection();
        self.reconcile_copy();
        let wrapped = self.metrics.wrapped();
        let built = self.metrics.lines_built();
        self.metrics.begin_frame();
        let native_output = self.metrics.math() == crate::math::MathPresentation::Native;
        if native_output {
            output(terminal.backend_mut(), crate::math::NativeStage::Begin)?;
        }

        let Self {
            state,
            metrics,
            palette,
            native,
            ..
        } = self;
        let mut drawn = SurfaceTree::default();
        let mut next_native = crate::math::NativeFrame::default();
        terminal.draw(|frame| {
            drawn = render(frame, state, palette, metrics);
            next_native = metrics.native_frame(frame, &drawn, state, palette);
            next_native.mark_diff(native, frame.buffer_mut());
        })?;
        if native_output {
            output(
                terminal.backend_mut(),
                crate::math::NativeStage::End {
                    changed: &next_native.changed(native),
                    current: next_native.text(),
                },
            )?;
        }

        self.native = next_native;
        self.surfaces = drawn;
        self.metrics.commit_frame();
        self.painted = Some(self.state.revision());
        self.painted_approval = self.approval_frame();
        self.frames = self.frames.saturating_add(1);
        Ok(Some(FrameWork {
            entries_wrapped: self.metrics.wrapped().saturating_sub(wrapped),
            lines_built: self.metrics.lines_built().saturating_sub(built),
        }))
    }
}
