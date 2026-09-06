use super::*;
use std::io::{self, Write};

use plexmaton_core::{AgentId, AgentStatus, EventSequence, TranscriptItemId, TranscriptRole};
use plexmaton_tui::{SurfaceId, TranscriptEntryView};
use ratatui::{
    TerminalOptions, Viewport,
    backend::{CrosstermBackend, TestBackend},
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::Rect,
};

struct Fixture {
    workspace: Workspace,
    terminal: Terminal<TestBackend>,
    frames: StreamFrames,
    now: Instant,
    sequence: u64,
    revision: u64,
    agent: AgentId,
    item: TranscriptItemId,
}

impl Fixture {
    fn new(width: u16, source: &str) -> Self {
        let now = Instant::now();
        let mut fixture = Self {
            workspace: Workspace::default(),
            terminal: Terminal::new(TestBackend::new(width, 24)).expect("terminal"),
            frames: StreamFrames::new(now),
            now,
            sequence: 0,
            revision: 0,
            agent: AgentId::new("primary").expect("agent"),
            item: TranscriptItemId::new("stream").expect("item"),
        };
        fixture.emit(ConversationEvent::AgentCreated {
            agent_id: fixture.agent.clone(),
            label: "Plexmaton".into(),
            status: AgentStatus::Running,
        });
        fixture.emit(ConversationEvent::TranscriptItemStarted {
            agent_id: fixture.agent.clone(),
            item_id: fixture.item.clone(),
            role: TranscriptRole::Assistant,
        });
        fixture.delta(source.into());
        assert!(fixture.draw(0).is_some());
        fixture
    }

    fn emit(&mut self, event: ConversationEvent) {
        self.sequence += 1;
        self.frames.receive(
            &mut self.workspace,
            ConversationEventEnvelope {
                sequence: EventSequence::new(self.sequence),
                event,
            },
        );
    }

    fn delta(&mut self, text: String) {
        self.revision += 1;
        self.emit(ConversationEvent::TranscriptDelta {
            agent_id: self.agent.clone(),
            item_id: self.item.clone(),
            item_revision: self.revision,
            text,
        });
    }

    fn draw(&mut self, millis: u64) -> Option<FrameWork> {
        let wrapped = self.workspace.metrics().wrapped();
        let built = self.workspace.metrics().lines_built();
        let drawn = self
            .frames
            .draw(
                &mut self.workspace,
                &mut self.terminal,
                self.now + Duration::from_millis(millis),
            )
            .expect("draw");
        if drawn.is_some() {
            // Pure component fixture at the external preparation boundary. Timer tests still call
            // StreamFrames first; neither production draw nor handle executes this parser.
            for attempt in 0..32 {
                let Some(work) = self.workspace.take_preparation() else {
                    break;
                };
                assert!(attempt < 31, "fixture preparation did not settle");
                assert!(
                    self.workspace.complete_preparation(
                        work.token,
                        work.requests
                            .iter()
                            .map(plexmaton_tui::preparation::Request::prepare)
                            .collect()
                    )
                );
                self.workspace
                    .draw(&mut self.terminal)
                    .expect("prepared frame");
            }
        }
        drawn.map(|_| FrameWork {
            entries_wrapped: self.workspace.metrics().wrapped() - wrapped,
            lines_built: self.workspace.metrics().lines_built() - built,
        })
    }

    fn handle(&mut self, event: &Event) -> Outcome {
        self.frames.handle(&mut self.workspace, event)
    }

    fn source(&self) -> &str {
        match self
            .workspace
            .state()
            .primary_agent()
            .expect("agent")
            .entries()
            .next()
            .expect("entry")
        {
            TranscriptEntryView::Text(item) => &item.source,
            other => panic!("expected text, got {other:?}"),
        }
    }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn mouse(kind: MouseEventKind, (column, row): (u16, u16)) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn point(terminal: &Terminal<TestBackend>, needle: &str) -> (u16, u16) {
    let buffer = terminal.backend().buffer();
    let needle: Vec<_> = needle.chars().map(|c| c.to_string()).collect();
    let length = u16::try_from(needle.len()).expect("short ASCII witness");
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width.saturating_sub(length) {
            if (0..length)
                .all(|offset| buffer[(x + offset, y)].symbol() == needle[usize::from(offset)])
            {
                return (x, y);
            }
        }
    }
    panic!("{needle:?} is not visible");
}

/// FR-1/FR-5: repeated arrivals do not restart the deadline, parse unseen text or leave an idle tick.
#[test]
fn stream_deadline_is_fixed_and_idle_owns_no_wake() {
    let mut fixture = Fixture::new(88, "**start** ");
    let layouts = fixture.workspace.metrics().text_layouts();
    for millis in 1..16 {
        fixture.delta("x".into());
        assert_eq!(
            fixture.frames.deadline(),
            Some(fixture.now + FRAME_INTERVAL)
        );
        assert_eq!(fixture.draw(millis), None);
        assert_eq!(fixture.source(), "**start** ");
        assert_eq!(fixture.workspace.metrics().text_layouts(), layouts);
    }
    let work = fixture.draw(16).expect("due stream frame");
    assert_eq!(work.entries_wrapped, 1);
    assert_eq!(fixture.workspace.metrics().text_layouts(), layouts + 1);
    assert_eq!(fixture.source(), format!("**start** {}", "x".repeat(15)));
    assert_eq!(fixture.frames.deadline(), None);
    for millis in [17, 32, 1_000] {
        assert_eq!(fixture.draw(millis), None);
    }
    // After idle, the first delta need not pay an additional interval.
    fixture.delta(" tail".into());
    assert!(fixture.draw(1_001).is_some());
    assert_eq!(fixture.frames.deadline(), None);
}

/// FR-5/MD-4: pressure and finalization flush every revision, but parse a rich stream three times.
#[test]
fn stream_event_pressure_bounds_batches_and_finalization_flushes_the_tail() {
    let mut fixture = Fixture::new(88, "# Stream\n\n");
    let frames = fixture.workspace.frames();
    let layouts = fixture.workspace.metrics().text_layouts();
    let mut expected = fixture.source().to_owned();
    for _ in 0..160 {
        let text = "**bold** ";
        expected.push_str(text);
        fixture.delta(text.into());
        assert!(fixture.frames.pending.len() < MAX_EVENTS);
        let _work = fixture.draw(1);
    }
    assert_eq!(
        fixture.workspace.frames() - frames,
        4,
        "two projection frames and their preparation completions"
    );
    assert_eq!(fixture.frames.pending.len(), 32);
    fixture.revision += 1;
    fixture.emit(ConversationEvent::TranscriptItemFinalized {
        agent_id: fixture.agent.clone(),
        item_id: fixture.item.clone(),
        item_revision: fixture.revision,
    });
    assert!(fixture.draw(2).is_some());
    assert_eq!(fixture.workspace.frames() - frames, 6);
    assert_eq!(fixture.workspace.metrics().text_layouts() - layouts, 3);
    assert_eq!(fixture.source(), expected);
    let entry = fixture
        .workspace
        .state()
        .primary_agent()
        .expect("agent")
        .entries()
        .next()
        .expect("entry");
    assert_eq!(entry.revision(), fixture.revision);
    assert!(matches!(entry, TranscriptEntryView::Text(item) if item.finalized));
    assert_eq!(fixture.frames.deadline(), None);
}

/// FR-5: small text with oversized retained allocation cannot bypass the presentation byte bound.
#[test]
fn stream_byte_pressure_counts_capacity_and_does_not_drop_oversized_events() {
    let mut fixture = Fixture::new(88, "**start** ");
    let mut first = String::with_capacity(MAX_TEXT_BYTES / 2 + 1);
    first.push('a');
    let capacity = first.capacity();
    fixture.delta(first);
    assert_eq!(fixture.frames.text_bytes, capacity);
    assert_eq!(fixture.source(), "**start** ");
    let mut second = String::with_capacity(MAX_TEXT_BYTES / 2 + 1);
    second.push('b');
    fixture.delta(second);
    assert!(fixture.frames.text_bytes <= MAX_TEXT_BYTES);
    assert_eq!(
        fixture.source(),
        "**start** a",
        "previous batch applied before admitting more"
    );
    assert!(fixture.draw(1).is_some());
    assert_eq!(fixture.source(), "**start** ab");

    fixture.delta("c".into());
    let mut oversized = String::with_capacity(MAX_TEXT_BYTES + 1);
    oversized.push('d');
    fixture.delta(oversized);
    assert_eq!(fixture.frames.text_bytes, 0);
    assert_eq!(fixture.frames.deadline(), None);
    assert_eq!(fixture.source(), "**start** abcd");
    assert!(fixture.draw(2).is_some());
}

/// FR-5/INV-7: typed input and its interrupt route are not delayed until the background deadline.
#[test]
fn input_and_interrupt_flush_streams_without_waiting_for_the_frame_interval() {
    for width in [120, 88, 60] {
        let mut fixture = Fixture::new(width, "**start**");
        let composer = fixture
            .workspace
            .surfaces()
            .get(SurfaceId::Composer)
            .expect("composer")
            .bounds;
        fixture.handle(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            (composer.x + 1, composer.y + 1),
        ));
        let _focus_frame = fixture.draw(0);
        fixture.delta(" first".into());
        fixture.handle(&key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(fixture.workspace.state().composer().text(), "x");
        assert!(fixture.draw(1).is_some());
        assert_eq!(fixture.source(), "**start** first");

        let clear = fixture.handle(&key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(clear.interrupted, None);
        assert!(fixture.draw(2).is_some());
        fixture.delta(" second".into());
        let interrupt = fixture.handle(&key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(interrupt.interrupted, Some(fixture.agent.clone()));
        assert!(fixture.draw(3).is_some());
        assert_eq!(fixture.source(), "**start** first second");
        assert_eq!(fixture.frames.deadline(), None);
    }
}

/// FR-3/FR-5/SEL-2: a queued closing delimiter must not reinterpret the map used by mouse release.
#[test]
fn stream_copy_uses_the_painted_markdown_before_applying_queued_delimiters() {
    for width in [120, 88, 60] {
        let mut fixture = Fixture::new(width, "Before **bold");
        let (x, y) = point(&fixture.terminal, "bold");
        fixture.handle(&mouse(MouseEventKind::Down(MouseButton::Left), (x, y)));
        fixture.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), (x + 4, y)));
        assert!(fixture.draw(1).is_some());
        fixture.delta("** after".into());
        assert_eq!(fixture.draw(2), None);
        let copied = fixture
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), (x + 4, y)))
            .copied
            .expect("copy the selected, still-painted word");
        assert_eq!(copied.text, "bold");
        assert_eq!(fixture.source(), "Before **bold** after");
        assert!(fixture.draw(3).is_some());
        assert!(
            fixture.workspace.state().selection().is_none(),
            "reinterpreted prefix invalidates the retained selection"
        );
    }
}

/// FR-3/FR-5: resize publishes neither guessed surfaces nor a stale pending stream frame.
#[test]
fn resize_flushes_pending_text_and_replaces_geometry_only_after_drawing() {
    let mut fixture = Fixture::new(120, "**start**");
    for (millis, width) in [(1, 88), (2, 60), (3, 120)] {
        let old = fixture
            .workspace
            .surfaces()
            .get(SurfaceId::Transcript)
            .expect("surface")
            .bounds;
        fixture.delta(" more words".into());
        fixture.terminal.backend_mut().resize(width, 24);
        fixture.handle(&Event::Resize(width, 24));
        assert_eq!(
            fixture
                .workspace
                .surfaces()
                .get(SurfaceId::Transcript)
                .expect("drawn surface")
                .bounds,
            old
        );
        assert!(fixture.draw(millis).is_some());
        assert_eq!(
            fixture
                .workspace
                .surfaces()
                .get(SurfaceId::Transcript)
                .expect("new surface")
                .bounds
                .width,
            width
        );
        assert_eq!(fixture.frames.deadline(), None);
    }
}

/// FR-5: explicit completion/report boundaries flush trailing text even before its timer is due.
#[test]
fn explicit_flush_retains_the_final_partial_stream_without_another_arrival() {
    let mut fixture = Fixture::new(88, "**start**");
    fixture.delta(" final fragment".into());
    fixture.frames.flush(&mut fixture.workspace);
    assert!(fixture.draw(1).is_some());
    assert_eq!(fixture.source(), "**start** final fragment");
    assert_eq!(fixture.frames.deadline(), None);
    fixture.frames.flush(&mut fixture.workspace);
    assert_eq!(fixture.draw(2), None);
}

struct BrokenOutput;

impl Write for BrokenOutput {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::BrokenPipe))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::BrokenPipe))
    }
}

/// FR-1/FR-3/FR-5: output failure retains source once, the old registry and an unadvanced deadline.
#[test]
fn failed_stream_frame_does_not_acknowledge_paint_or_replay_its_deltas() {
    let mut fixture = Fixture::new(88, "**start**");
    fixture.delta(" retained".into());
    let painted = fixture.workspace.frames();
    let old_surface = fixture
        .workspace
        .surfaces()
        .get(SurfaceId::Transcript)
        .expect("surface")
        .bounds;
    let previous_deadline = fixture.frames.not_before;
    let mut broken = Terminal::with_options(
        CrosstermBackend::new(BrokenOutput),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 60, 24)),
        },
    )
    .expect("fixed viewport requires no real terminal");
    let result = fixture.frames.draw(
        &mut fixture.workspace,
        &mut broken,
        fixture.now + FRAME_INTERVAL,
    );
    assert_eq!(
        result.expect_err("broken terminal write").kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(fixture.source(), "**start** retained");
    assert_eq!(fixture.workspace.frames(), painted);
    assert!(fixture.workspace.needs_draw());
    assert_eq!(fixture.frames.not_before, previous_deadline);
    assert_eq!(
        fixture
            .workspace
            .surfaces()
            .get(SurfaceId::Transcript)
            .expect("old surface")
            .bounds,
        old_surface
    );
    assert!(fixture.draw(17).is_some());
    assert_eq!(fixture.source(), "**start** retained");
    assert_eq!(
        fixture.workspace.frames(),
        painted + 2,
        "recovered projection and prepared result both paint"
    );
    assert_eq!(fixture.frames.deadline(), None);
}
