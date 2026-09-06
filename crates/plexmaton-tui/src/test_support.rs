//! Fixtures shared by this crate's unit tests.
//!
//! Every helper here had two real callers before it moved in. A fixture with one caller stays
//! beside the test that uses it, because a shared fixture nobody else needs is indirection with no
//! payer.

use plexmaton_core::{
    AgentId, EventSequence, SessionEvent, SessionEventEnvelope, ToolCallId, ToolCallStatus,
    ToolPresentation, TranscriptItemId, TranscriptRole,
};
use plexmaton_sim::{Scenario, ScriptedRuntime};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};

use crate::{
    TranscriptMetrics, ViewState,
    intent::ScrollDirection,
    render,
    surface::{SurfaceId, SurfaceTree, Viewport},
    theme::Palette,
};

/// A runtime holding the canonical A-delegates-to-B timeline, with nothing emitted yet.
pub fn canonical_runtime() -> ScriptedRuntime {
    ScriptedRuntime::new(
        Scenario::canonical().unwrap_or_else(|error| panic!("invalid fixture: {error}")),
    )
}

/// Credential-free active-model display used by configuration interaction and frame fixtures.
pub fn configuration_summary() -> crate::ConfigurationSummary {
    crate::ConfigurationSummary {
        provider: "local".to_owned(),
        model: "gpt-5.6-sol".to_owned(),
        reasoning_effort: "high".to_owned(),
    }
}

/// The canonical timeline, fully replayed into a projection.
pub fn canonical_state() -> ViewState {
    let mut state = Conversation::canonical().state;
    state.set_working_directory("~/plexmaton".to_owned());
    state
}

/// The canonical projection while the primary agent has an open assistant response.
pub fn current_responding_state() -> ViewState {
    let mut conversation = Conversation::canonical();
    conversation.emit(SessionEvent::TranscriptItemStarted {
        agent_id: conversation.agent.clone(),
        item_id: TranscriptItemId::new("current-response")
            .unwrap_or_else(|error| panic!("invalid fixture: {error}")),
        role: TranscriptRole::Assistant,
    });
    conversation.state
}

/// The canonical projection while the primary agent is running one tool.
pub fn current_running_tool_state() -> ViewState {
    let mut conversation = Conversation::canonical();
    let item_id = TranscriptItemId::new("current-running-tool")
        .unwrap_or_else(|error| panic!("invalid fixture: {error}"));
    let call_id = ToolCallId::new("current-running-tool")
        .unwrap_or_else(|error| panic!("invalid fixture: {error}"));
    for (item_revision, status) in [(0, ToolCallStatus::Queued), (1, ToolCallStatus::Running)] {
        conversation.emit(SessionEvent::ToolCallChanged {
            agent_id: conversation.agent.clone(),
            item_id: item_id.clone(),
            item_revision,
            call_id: call_id.clone(),
            label: "read_file".to_owned(),
            status,
            presentation: ToolPresentation::default(),
        });
    }
    conversation.state
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
    pending: Vec<SessionEventEnvelope>,
}

impl Conversation {
    /// The canonical timeline, replayed, holding the stream position it left off at.
    pub fn canonical() -> Self {
        let mut state = ViewState::default();
        let mut sequence = 0;
        let mut pending = Vec::new();
        // A tick past the end of the timeline, so everything scheduled has been emitted.
        for envelope in canonical_runtime().ready(u64::MAX) {
            sequence = envelope.sequence.get();
            pending.push(envelope.clone());
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
            pending,
        }
    }

    /// Takes the envelopes emitted since the last call, for a caller with its own projection.
    ///
    /// A [`Workspace`](crate::Workspace) reduces the stream itself, so a test driving one has to be
    /// handed the events rather than the finished state. Both projections then see the same stream
    /// in the same order, which is the only way their revisions stay comparable.
    pub fn drain(&mut self) -> Vec<SessionEventEnvelope> {
        std::mem::take(&mut self.pending)
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
            self.emit(SessionEvent::TranscriptItemStarted {
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
        self.emit(SessionEvent::TranscriptDelta {
            agent_id: agent_id.clone(),
            item_id: item_id.clone(),
            item_revision,
            text: text.to_owned(),
        });
        self.newest = Some((agent_id, item_id, item_revision));
        self
    }

    /// Emits one event at the next sequence number, and insists the projection accepted it.
    ///
    /// Public because the counter is the reason this fixture exists: a test that builds its own
    /// envelope has to guess the sequence, and a guess produces a notice log rather than a
    /// transcript, with an assertion failure that says nothing about why.
    pub fn emit(&mut self, event: SessionEvent) {
        self.sequence = self.sequence.saturating_add(1);
        let envelope = SessionEventEnvelope {
            sequence: EventSequence::new(self.sequence),
            event,
        };
        self.pending.push(envelope.clone());
        let outcome = self.state.apply(envelope);
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
    state.apply(SessionEventEnvelope {
        // A stale sequence: the canonical scenario has already advanced well past 1.
        sequence: EventSequence::new(1),
        event: SessionEvent::RuntimeWarning {
            agent_id: AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            item_id: TranscriptItemId::new("stale-warning")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
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
    for attempt in 0..512 {
        metrics.begin_frame();
        terminal
            .draw(|frame| surfaces = render(frame, state, palette, metrics))
            .expect("test render");
        metrics.commit_frame();
        let requests: Vec<_> = metrics
            .preparation_needed()
            .iter()
            .filter_map(|key| {
                let item = state
                    .agent(&key.agent)?
                    .entries()
                    .find(|entry| entry.id() == &key.item)?;
                Some(crate::preparation::Request::new(
                    key.agent.clone(),
                    item.clone(),
                    key.width,
                    key.open,
                ))
            })
            .collect();
        if requests.is_empty() {
            break;
        }
        assert!(attempt < 511, "prepared fixture did not settle");
        for request in requests {
            metrics.accept_prepared(request.prepare());
        }
    }
    (surfaces, terminal.backend().buffer().clone())
}

impl crate::Workspace {
    /// Explicit component-fixture pump at the public preparation boundary. Production `draw`
    /// never calls a parser, including in test builds. Pending/failure tests use `draw` directly.
    pub(crate) fn settled_draw<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<Option<crate::FrameWork>, B::Error> {
        let before_wrapped = self.metrics().wrapped();
        let before_built = self.metrics().lines_built();
        let before_frames = self.frames();
        for attempt in 0..1024 {
            self.draw(terminal)?;
            if let Some(work) = self.take_preparation() {
                assert!(attempt < 1023, "prepared fixture did not settle");
                match crate::preparation::prepare_batch(&work.requests) {
                    Ok(prepared) => assert!(
                        self.complete_preparation(work.token, prepared),
                        "fixture reply rejected"
                    ),
                    Err(crate::preparation::BatchRefusal::Capacity) => {
                        self.fail_preparation(work.token, crate::preparation::Refusal::Capacity)
                    }
                }
            } else if !self.needs_draw() {
                return Ok((before_frames != self.frames()).then(|| crate::FrameWork {
                    entries_wrapped: self.metrics().wrapped() - before_wrapped,
                    lines_built: self.metrics().lines_built() - before_built,
                }));
            }
        }
        panic!("prepared fixture did not settle");
    }
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

/// Snapshot text follows display cells, skips wide continuations, and trims only terminal padding.
/// Geometry assertions use `region_text` or the buffer itself; this readable projection is not copy.
pub fn snapshot_text(buffer: &Buffer, bounds: Rect) -> String {
    use unicode_width::UnicodeWidthStr as _;

    assert_eq!(
        bounds.intersection(buffer.area),
        bounds,
        "snapshot bounds exceed the buffer"
    );
    let mut text = String::new();
    for y in bounds.top()..bounds.bottom() {
        let mut row = String::new();
        let mut x = bounds.left();
        while x < bounds.right() {
            let symbol = buffer[(x, y)].symbol();
            row.push_str(symbol);
            x += symbol.width().max(1) as u16;
        }
        text.push_str(row.trim_end_matches(' '));
        text.push('\n');
    }
    text
}

/// Compare one named frame with a compact first-difference diagnostic; writes remain opt-in.
pub fn assert_frame(name: &str, drawn: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("frames")
        .join(format!("{name}.txt"));
    if std::env::var_os("PLEXMATON_WRITE_FRAMES").is_some() {
        std::fs::write(&path, drawn)
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }
    let fixture = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "read {}: {error}; use PLEXMATON_WRITE_FRAMES=1 and review the diff",
            path.display()
        )
    });
    assert!(
        fixture == drawn,
        "{name}: {}; refresh with PLEXMATON_WRITE_FRAMES=1 and review the diff",
        first_difference(&fixture, drawn)
    );
}

fn first_difference(expected: &str, actual: &str) -> String {
    for (index, (want, got)) in expected.lines().zip(actual.lines()).enumerate() {
        if want != got {
            return format!(
                "line {}:\n  fixture: {want:?}\n  drawn:   {got:?}",
                index + 1
            );
        }
    }
    format!(
        "line count: fixture {}, drawn {}; final newline: fixture {}, drawn {}",
        expected.lines().count(),
        actual.lines().count(),
        expected.ends_with('\n'),
        actual.ends_with('\n')
    )
}

#[test]
fn snapshot_projection_keeps_wide_graphemes_combining_marks_and_right_edge_icons() {
    let mut buffer = Buffer::empty(Rect::new(2, 3, 10, 1));
    buffer.set_string(2, 3, "中e\u{301} 󰆏", ratatui::style::Style::default());
    assert_eq!(snapshot_text(&buffer, buffer.area), "中e\u{301} 󰆏\n");
    buffer[(11, 3)].set_symbol("󰆏");
    let with_icon = snapshot_text(&buffer, buffer.area);
    assert!(with_icon.ends_with("󰆏\n"));
    buffer[(11, 3)].set_symbol(" ");
    assert_ne!(snapshot_text(&buffer, buffer.area), with_icon);
    assert!(first_difference("same\n", "same").contains("final newline"));
}
