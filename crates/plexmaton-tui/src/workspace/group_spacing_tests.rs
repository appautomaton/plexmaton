use super::*;
use crate::{Point, SurfaceId};
use plexmaton_core::{
    AgentStatus, ConversationEvent, EventSequence, ToolCallId, ToolCallStatus, ToolPresentation,
    TranscriptItemId, TranscriptRole,
};
use ratatui::{
    backend::TestBackend,
    crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};

enum Entry<'a> {
    Tool(&'a str),
    Text(TranscriptRole, &'a str),
}

fn append(workspace: &mut Workspace, sequence: &mut u64, entries: &[Entry<'_>]) {
    let agent = AgentId::new("primary").expect("agent");
    for entry in entries {
        let item = TranscriptItemId::new(format!("entry-{sequence}")).expect("item");
        let events = match entry {
            Entry::Tool(label) => [
                ToolCallStatus::Queued,
                ToolCallStatus::Running,
                ToolCallStatus::Succeeded,
            ]
            .into_iter()
            .enumerate()
            .map(|(revision, status)| ConversationEvent::ToolCallChanged {
                agent_id: agent.clone(),
                item_id: item.clone(),
                item_revision: revision as u64,
                call_id: ToolCallId::new(format!("call-{sequence}")).expect("call"),
                label: (*label).into(),
                status,
                presentation: ToolPresentation {
                    invocation: Some(plexmaton_core::ToolDetail::Text {
                        source: "Retained invocation.\nSecond detail line.".into(),
                        omitted_bytes: 0,
                    }),
                    outcome: None,
                },
            })
            .collect(),
            Entry::Text(role, source) => vec![
                ConversationEvent::TranscriptItemStarted {
                    agent_id: agent.clone(),
                    item_id: item.clone(),
                    role: *role,
                },
                ConversationEvent::TranscriptDelta {
                    agent_id: agent.clone(),
                    item_id: item,
                    item_revision: 1,
                    text: (*source).into(),
                },
            ],
        };
        workspace.emit(
            events
                .into_iter()
                .map(|event| {
                    let envelope = ConversationEventEnvelope {
                        sequence: EventSequence::new(*sequence),
                        event,
                    };
                    *sequence += 1;
                    envelope
                })
                .collect(),
        );
    }
}

fn fixture(width: u16, entries: &[Entry<'_>]) -> (Workspace, Terminal<TestBackend>, u64) {
    let mut workspace = Workspace::with_palette(Palette::pastel());
    workspace.emit(vec![ConversationEventEnvelope {
        sequence: EventSequence::new(1),
        event: ConversationEvent::AgentCreated {
            agent_id: AgentId::new("primary").expect("agent"),
            label: "Plexmaton".into(),
            status: AgentStatus::Idle,
        },
    }]);
    let mut sequence = 2;
    append(&mut workspace, &mut sequence, entries);
    let mut terminal = Terminal::new(TestBackend::new(width, 32)).expect("terminal");
    workspace.settled_draw(&mut terminal).expect("draw");
    (workspace, terminal, sequence)
}

fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

fn point(terminal: &Terminal<TestBackend>, needle: &str) -> Point {
    for y in 0..terminal.backend().buffer().area.height {
        if let Some(x) = row(terminal, y).find(needle) {
            use unicode_width::UnicodeWidthStr as _;
            return Point {
                x: row(terminal, y)[..x].width() as u16,
                y,
            };
        }
    }
    panic!("{needle:?} not visible");
}

fn mouse(kind: MouseEventKind, at: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: at.x,
        row: at.y,
        modifiers: KeyModifiers::NONE,
    })
}

/// TR-6/SEL-7/ENT-1: groups share one separator; hovering consumes it without covering a tool.
#[test]
fn transcript_group_boundaries_share_spacing_and_hover_geometry() {
    for width in [60, 95, 120] {
        let source = "Checking once.\n\n\n";
        let (mut workspace, mut terminal, _) = fixture(
            width,
            &[
                Entry::Tool("first_tool"),
                Entry::Tool("second_tool"),
                Entry::Text(TranscriptRole::Reasoning, source),
                Entry::Text(TranscriptRole::Assistant, "First answer."),
                Entry::Tool("third_tool"),
                Entry::Text(TranscriptRole::Assistant, "Last answer."),
            ],
        );
        let first_tool = point(&terminal, "first_tool");
        let second_tool = point(&terminal, "second_tool");
        let heading = point(&terminal, "reasoning");
        let thought = point(&terminal, "Checking once.");
        let first_answer = point(&terminal, "First answer.");
        let third_tool = point(&terminal, "third_tool");
        let last_answer = point(&terminal, "Last answer.");
        assert_eq!(
            second_tool.y,
            first_tool.y + 1,
            "tools form a compact group"
        );
        assert_eq!(
            heading.y,
            second_tool.y + 2,
            "one tool-to-message separator"
        );
        assert_eq!(
            first_answer.y,
            thought.y + 2,
            "one message-to-message separator"
        );
        assert_eq!(
            third_tool.y,
            first_answer.y + 2,
            "one message-to-tool separator"
        );
        assert_eq!(
            last_answer.y,
            third_tool.y + 2,
            "all message roles share the rule"
        );
        let before_tool = row(&terminal, second_tool.y);
        let wraps = workspace.metrics.wrapped();
        workspace.handle(&mouse(MouseEventKind::Moved, thought));
        workspace.settled_draw(&mut terminal).expect("hover");
        assert!(row(&terminal, heading.y - 1).contains("────"));
        assert!(row(&terminal, thought.y + 1).contains("────"));
        assert_eq!(row(&terminal, second_tool.y), before_tool);
        assert_eq!(point(&terminal, "Checking once."), thought);
        assert_eq!(
            workspace.metrics.wrapped(),
            wraps,
            "hover does not remeasure"
        );
        let icon = point(&terminal, "󰆏");
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), icon));
        assert_eq!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), icon))
                .copied
                .expect("source copy")
                .text,
            source
        );
        let agent = AgentId::new("primary").expect("agent");
        let viewport = workspace
            .surfaces
            .viewport(SurfaceId::Transcript)
            .expect("viewport");
        assert_eq!(
            workspace.metrics.total_rows(&agent, viewport.content_width),
            12
        );
    }
}

/// TR-1/TR-6: a new message closes the previous tool group without rewrapping old entries.
#[test]
fn closing_a_tool_group_changes_spacing_without_rewrapping_its_body() {
    let (mut workspace, mut terminal, mut sequence) =
        fixture(95, &[Entry::Tool("first_tool"), Entry::Tool("second_tool")]);
    let wraps = workspace.metrics.wrapped();
    append(
        &mut workspace,
        &mut sequence,
        &[Entry::Text(TranscriptRole::Reasoning, "A new thought.")],
    );
    workspace.settled_draw(&mut terminal).expect("new message");
    assert_eq!(workspace.metrics.wrapped() - wraps, 1);
    assert_eq!(
        point(&terminal, "reasoning").y,
        point(&terminal, "second_tool").y + 2
    );
}

/// TR-6/SEL-2: composition-only rows are caret edges, not copied newlines or tool click targets.
#[test]
fn group_separator_drag_copies_visible_text_in_both_directions() {
    for width in [60, 95, 120] {
        for reverse in [false, true] {
            let (mut workspace, mut terminal, _) = fixture(
                width,
                &[
                    Entry::Tool("read_file"),
                    Entry::Text(TranscriptRole::Reasoning, "Checking once.\n\n\n"),
                    Entry::Text(TranscriptRole::Assistant, "Answer follows."),
                ],
            );
            let start = point(&terminal, "Checking once.");
            let answer = point(&terminal, "Answer follows.");
            let end = Point {
                x: answer.x,
                y: answer.y + 1,
            };
            let (start, end) = if reverse { (end, start) } else { (start, end) };
            workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), start));
            workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), end));
            let copied = workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), end));
            assert_eq!(
                copied.copied.expect("selected visible text").text,
                "Checking once.\n\nAnswer follows."
            );
            workspace.settled_draw(&mut terminal).expect("selection");
            let tool = point(&terminal, "read_file");
            let gap = Point {
                x: tool.x,
                y: tool.y + 1,
            };
            workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), gap));
            workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), gap));
            assert!(
                !workspace.state.disclosure().is_open(
                    workspace
                        .state
                        .primary_agent()
                        .expect("agent")
                        .entries()
                        .next()
                        .expect("tool")
                        .id()
                )
            );
        }
    }
}

/// TR-2/TR-3/TR-6: every clipped mixed-group window matches full composition and round-trips anchors.
#[test]
fn mixed_group_windows_and_anchors_share_the_composed_rows() {
    let (mut workspace, mut terminal, _) = fixture(
        95,
        &[
            Entry::Tool("first_tool"),
            Entry::Tool("second_tool"),
            Entry::Text(
                TranscriptRole::Reasoning,
                "A thought long enough to wrap at the narrow geometry, without extra source padding.",
            ),
            Entry::Text(TranscriptRole::Assistant, "Answer follows."),
            Entry::Tool("third_tool"),
            Entry::Text(TranscriptRole::Assistant, "Done."),
        ],
    );
    for width in [58, 93, 118, 58] {
        terminal.backend_mut().resize(width + 2, 32);
        terminal
            .resize(ratatui::layout::Rect::new(0, 0, width + 2, 32))
            .expect("resize");
        workspace.handle(&Event::Resize(width + 2, 32));
        workspace.settled_draw(&mut terminal).expect("reflow");
        let agent = workspace.state.primary_agent().expect("agent");
        workspace.metrics.measure(agent, &workspace.palette, width);
        let whole = crate::test_support::conversation_lines(agent, &workspace.palette, width);
        let total = workspace.metrics.total_rows(&agent.id, width);
        assert_eq!(total, whole.len());
        for offset in 0..total {
            let anchor = workspace
                .metrics
                .anchor_at(&agent.id, width, offset)
                .expect("anchor");
            assert_eq!(
                workspace
                    .metrics
                    .offset_of(&agent.id, width, &anchor, total),
                offset
            );
            let window = workspace.metrics.window(&agent.id, width, offset, 3);
            let (lines, skip) = workspace.metrics.build(
                agent,
                &workspace.palette,
                &window,
                &workspace.state,
                SurfaceId::Transcript,
            );
            let visible: Vec<_> = lines
                .iter()
                .skip(usize::from(skip))
                .take(3)
                .map(ToString::to_string)
                .collect();
            let expected: Vec<_> = whole
                .iter()
                .skip(offset)
                .take(3)
                .map(ToString::to_string)
                .collect();
            assert_eq!(visible, expected, "width {width}, offset {offset}");
        }
    }
}

/// TR-6: a tool's recovery receipt supplies the closing separator; opening keeps it single.
#[test]
fn tool_feedback_closes_the_group_without_a_second_separator() {
    for width in [60, 95, 120] {
        let (mut workspace, mut terminal, mut sequence) =
            fixture(width, &[Entry::Tool("read_file")]);
        workspace
            .state
            .report_conversation_recovery(ConversationRestoration { tail: None });
        append(
            &mut workspace,
            &mut sequence,
            &[Entry::Text(TranscriptRole::Reasoning, "Continue.")],
        );
        workspace.settled_draw(&mut terminal).expect("restored");
        for open in [false, true] {
            if open {
                let tool = point(&terminal, "read_file");
                workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), tool));
                workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), tool));
                workspace.settled_draw(&mut terminal).expect("disclose");
            }
            let tool_id = workspace
                .state
                .primary_agent()
                .expect("agent")
                .entries()
                .next()
                .expect("tool")
                .id();
            assert_eq!(workspace.state.disclosure().is_open(tool_id), open);
            if open {
                point(&terminal, "Second detail line.");
            }
            let receipt = point(&terminal, "Conversation restored");
            assert_eq!(point(&terminal, "reasoning").y, receipt.y + 2);
            assert!(
                workspace
                    .text_point_at(SurfaceId::Transcript, receipt, false)
                    .is_none(),
                "feedback is never copied as tool text"
            );
        }
    }
}
