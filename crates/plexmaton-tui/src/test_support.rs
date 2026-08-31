//! Fixtures shared by this crate's unit tests.
//!
//! Every helper here had two real callers before it moved in. A fixture with one caller stays
//! beside the test that uses it, because a shared fixture nobody else needs is indirection with no
//! payer.

use plexmaton_core::{EventSequence, PrototypeEvent, PrototypeEventEnvelope};
use plexmaton_sim::Scenario;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};

use crate::{ViewState, render, surface::SurfaceTree, theme::Palette};

/// The canonical A-delegates-to-B timeline, fully applied.
pub fn canonical_state() -> ViewState {
    let scenario = Scenario::canonical().unwrap_or_else(|error| panic!("invalid fixture: {error}"));
    let mut state = ViewState::default();
    for step in scenario.into_steps() {
        state.apply(step.envelope);
    }
    state
}

/// The canonical timeline plus one producer defect, so the notice strip exists.
pub fn degraded_state() -> ViewState {
    let mut state = canonical_state();
    state.apply(PrototypeEventEnvelope {
        // A stale sequence: the canonical scenario has already advanced well past 1.
        sequence: EventSequence::new(1),
        event: PrototypeEvent::RuntimeWarning {
            message: "producer replayed an old event".into(),
        },
    });
    state
}

/// Draws one frame and returns the surfaces it registered alongside the painted cells.
pub fn draw_frame(
    state: &ViewState,
    palette: &Palette,
    width: u16,
    height: u16,
) -> (SurfaceTree, Buffer) {
    let mut terminal = Terminal::new(TestBackend::new(width, height))
        .unwrap_or_else(|error| panic!("test terminal: {error}"));
    let mut surfaces = SurfaceTree::default();
    terminal
        .draw(|frame| surfaces = render(frame, state, palette))
        .unwrap_or_else(|error| panic!("test render: {error}"));
    (surfaces, terminal.backend().buffer().clone())
}

/// Draws one frame with an explicit palette and returns it as text.
pub fn draw_with(state: &ViewState, palette: &Palette, width: u16, height: u16) -> String {
    buffer_text(&draw_frame(state, palette, width, height).1)
}

/// Draws one frame with the default palette and returns it as text.
pub fn draw(state: &ViewState, width: u16, height: u16) -> String {
    draw_with(state, &Palette::default(), width, height)
}

/// Returns the text painted inside one rectangle, one line per row.
///
/// Reading the buffer back through a rectangle is what makes "drawn equals registered" provable:
/// a panel painted somewhere other than the region it registered leaves its signature outside the
/// rectangle this returns.
pub fn region_text(buffer: &Buffer, bounds: Rect) -> String {
    let mut rows = Vec::with_capacity(usize::from(bounds.height));
    for y in bounds.top()..bounds.bottom() {
        let mut row = String::with_capacity(usize::from(bounds.width));
        for x in bounds.left()..bounds.right() {
            row.push_str(buffer[(x, y)].symbol());
        }
        rows.push(row);
    }
    rows.join("\n")
}

fn buffer_text(buffer: &Buffer) -> String {
    region_text(buffer, *buffer.area())
}
