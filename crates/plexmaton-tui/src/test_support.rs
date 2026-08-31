//! Fixtures shared by this crate's unit tests.
//!
//! Every helper here had two real callers before it moved in. A fixture with one caller stays
//! beside the test that uses it, because a shared fixture nobody else needs is indirection with no
//! payer.

use plexmaton_core::{
    AgentId, EventSequence, PrototypeEvent, PrototypeEventEnvelope, TranscriptItemId,
    TranscriptRole,
};
use plexmaton_sim::{Runtime, Scenario};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};

use crate::{
    TranscriptMetrics, ViewState,
    intent::ScrollDirection,
    render,
    surface::{SurfaceId, SurfaceTree, Viewport},
    theme::Palette,
};

/// A runtime holding the canonical A-delegates-to-B timeline, with nothing emitted yet.
pub fn canonical_runtime() -> Runtime {
    Runtime::new(Scenario::canonical().unwrap_or_else(|error| panic!("invalid fixture: {error}")))
}

/// The canonical timeline, fully replayed into a projection.
pub fn canonical_state() -> ViewState {
    Conversation::canonical().state
}

/// A projection a test can keep streaming into.
///
/// Sequence numbers are the projection's ordering contract, so a test that appends events has to
/// continue them from wherever the fixture stopped. This owns that counter, because a test that
/// guesses it gets a notice log instead of a transcript and the assertion failure says nothing
/// about why.
pub struct Conversation {
    pub state: ViewState,
    agent: AgentId,
    sequence: u64,
    items: usize,
    newest: Option<(AgentId, TranscriptItemId, u64)>,
}

impl Conversation {
    /// The canonical timeline, replayed, holding the stream position it left off at.
    pub fn canonical() -> Self {
        let mut state = ViewState::default();
        let mut sequence = 0;
        // A tick past the end of the timeline, so everything scheduled has been emitted.
        for envelope in canonical_runtime().ready(u64::MAX) {
            sequence = envelope.sequence.get();
            state.apply(envelope);
        }
        let agent = state
            .primary_agent()
            .map(|agent| agent.id.clone())
            .unwrap_or_else(|| panic!("the canonical timeline creates a primary agent"));
        Self {
            state,
            agent,
            sequence,
            items: 0,
            newest: None,
        }
    }

    /// Streams `count` further assistant items into the primary agent.
    pub fn extend(&mut self, count: usize) -> &mut Self {
        let agent = self.agent.clone();
        self.extend_agent(&agent, count)
    }

    /// Streams `count` assistant items into one agent, each long enough to wrap in a narrow panel.
    pub fn extend_agent(&mut self, agent_id: &AgentId, count: usize) -> &mut Self {
        for _ in 0..count {
            self.items = self.items.saturating_add(1);
            let item_id = TranscriptItemId::new(format!("filler-{}", self.items))
                .unwrap_or_else(|error| panic!("invalid fixture: {error}"));
            self.emit(PrototypeEvent::TranscriptItemStarted {
                agent_id: agent_id.clone(),
                item_id: item_id.clone(),
                role: TranscriptRole::Assistant,
            });
            self.newest = Some((agent_id.clone(), item_id, 0));
            self.append(&format!(
                "Filler message {} with enough words in it to wrap across several rows of a \
                 narrow panel.",
                self.items
            ));
        }
        self
    }

    /// Appends text to the newest streamed item, the way a live producer's delta would.
    pub fn append(&mut self, text: &str) -> &mut Self {
        let (agent_id, item_id, revision) = self
            .newest
            .clone()
            .unwrap_or_else(|| panic!("nothing has been streamed to append to"));
        let item_revision = revision.saturating_add(1);
        self.emit(PrototypeEvent::TranscriptDelta {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            item_revision,
            text: text.to_owned(),
        });
        self.newest = Some((agent_id, item_id, item_revision));
        self
    }

    fn emit(&mut self, event: PrototypeEvent) {
        self.sequence = self.sequence.saturating_add(1);
        let outcome = self.state.apply(PrototypeEventEnvelope {
            sequence: EventSequence::new(self.sequence),
            event,
        });
        assert!(
            matches!(outcome, crate::ApplyOutcome::Accepted),
            "the fixture emitted an event the projection rejected: {outcome:?}"
        );
    }
}

/// A projection, its wrapping cache, and a terminal, stepped the way the binary steps them.
///
/// Both halves matter. The cache has to outlive the frame or none of its invariants mean anything,
/// and the scroll path reads the same cache the frame wrote — a test that built a fresh one per
/// frame would be measuring a cold cache the binary never has.
pub struct Session {
    pub conversation: Conversation,
    palette: Palette,
    metrics: TranscriptMetrics,
    surfaces: SurfaceTree,
    buffer: Buffer,
    width: u16,
    height: u16,
}

impl Session {
    /// The canonical timeline on a terminal of this size, with one frame already drawn.
    pub fn canonical(width: u16, height: u16) -> Self {
        let mut session = Self {
            conversation: Conversation::canonical(),
            palette: Palette::default(),
            metrics: TranscriptMetrics::default(),
            surfaces: SurfaceTree::default(),
            buffer: Buffer::empty(Rect::new(0, 0, width, height)),
            width,
            height,
        };
        session.draw();
        session
    }

    /// Draws one frame into the retained cache and surface registry.
    pub fn draw(&mut self) -> &mut Self {
        let (surfaces, buffer) = draw_cached(
            &self.conversation.state,
            &self.palette,
            &mut self.metrics,
            self.width,
            self.height,
        );
        self.surfaces = surfaces;
        self.buffer = buffer;
        self
    }

    /// Resizes the terminal and redraws, the way a resize intent does.
    pub fn resize(&mut self, width: u16, height: u16) -> &mut Self {
        self.width = width;
        self.height = height;
        self.draw()
    }

    /// Turns the wheel over one surface, redrawing between notches as the event loop does.
    pub fn wheel(
        &mut self,
        surface_id: SurfaceId,
        direction: ScrollDirection,
        notches: usize,
    ) -> &mut Self {
        for _ in 0..notches {
            self.conversation
                .state
                .scroll(&self.surfaces, &self.metrics, surface_id, direction);
            self.draw();
        }
        self
    }

    /// Selects an agent and redraws.
    pub fn select(&mut self, agent_id: &AgentId) -> &mut Self {
        self.conversation
            .state
            .select_agent(agent_id)
            .unwrap_or_else(|error| panic!("the fixture selected an unknown agent: {error}"));
        self.draw()
    }

    /// The text painted inside one surface by the last frame.
    pub fn region(&self, surface_id: SurfaceId) -> String {
        region_text(&self.buffer, self.bounds(surface_id))
    }

    pub fn bounds(&self, surface_id: SurfaceId) -> Rect {
        self.surfaces
            .get(surface_id)
            .unwrap_or_else(|| panic!("{surface_id:?} must be registered"))
            .bounds
    }

    pub fn viewport(&self, surface_id: SurfaceId) -> Viewport {
        self.surfaces
            .viewport(surface_id)
            .unwrap_or_else(|| panic!("{surface_id:?} was drawn, so it has been measured"))
    }
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
    draw_cached(
        state,
        palette,
        &mut TranscriptMetrics::default(),
        width,
        height,
    )
}

/// Draws one frame against a cache that outlives it, the way the running binary does.
pub fn draw_cached(
    state: &ViewState,
    palette: &Palette,
    metrics: &mut TranscriptMetrics,
    width: u16,
    height: u16,
) -> (SurfaceTree, Buffer) {
    let mut terminal = Terminal::new(TestBackend::new(width, height))
        .unwrap_or_else(|error| panic!("test terminal: {error}"));
    let mut surfaces = SurfaceTree::default();
    terminal
        .draw(|frame| surfaces = render(frame, state, palette, metrics))
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
