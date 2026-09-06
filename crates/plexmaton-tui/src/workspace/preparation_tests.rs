use super::*;
use crate::{
    Point, SurfaceId,
    preparation::{Refusal, Request},
    state::CopyNote,
};
use plexmaton_core::{
    AgentStatus, ConversationEvent, EventSequence, TranscriptItemId, TranscriptRole,
};
use ratatui::{
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};
use unicode_width::UnicodeWidthStr as _;

fn fixture(width: u16, count: usize) -> (Workspace, Terminal<TestBackend>, u64) {
    let agent = AgentId::new("primary").expect("agent");
    let mut events = vec![ConversationEvent::AgentCreated {
        agent_id: agent.clone(),
        label: "Plexmaton".into(),
        status: AgentStatus::Running,
    }];
    for index in 0..count {
        let item = TranscriptItemId::new(format!("item-{index}")).expect("item");
        events.push(ConversationEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: item.clone(),
            role: TranscriptRole::Assistant,
        });
        events.push(ConversationEvent::TranscriptDelta {
            agent_id: agent.clone(),
            item_id: item,
            item_revision: 1,
            text: format!("**message {index:03}** 中文"),
        });
    }
    let sequence = events.len() as u64;
    let mut workspace = Workspace::default();
    workspace.emit(
        events
            .into_iter()
            .enumerate()
            .map(|(index, event)| ConversationEventEnvelope {
                sequence: EventSequence::new(index as u64 + 1),
                event,
            })
            .collect(),
    );
    (
        workspace,
        Terminal::new(TestBackend::new(width, 24)).expect("terminal"),
        sequence,
    )
}

fn point(terminal: &Terminal<TestBackend>, needle: &str) -> Point {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let text = crate::test_support::snapshot_text(
            buffer,
            ratatui::layout::Rect::new(0, y, buffer.area.width, 1),
        );
        if let Some(byte) = text.find(needle) {
            return Point {
                x: text[..byte].width() as u16,
                y,
            };
        }
    }
    panic!(
        "missing {needle:?}: {}",
        crate::test_support::snapshot_text(buffer, buffer.area)
    );
}

fn mouse(kind: MouseEventKind, at: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: at.x,
        row: at.y,
        modifiers: KeyModifiers::NONE,
    })
}

fn ctrl(letter: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::CONTROL))
}

/// ENT-2/MD-4: a tool transition is a replaced fact, not an append-only text prefix. Pending
/// preparation cannot continue to advertise the obsolete lifecycle state or its text hit map.
#[test]
fn tool_transitions_do_not_reuse_stale_prepared_status() {
    use plexmaton_core::{ToolCallId, ToolCallStatus, ToolPresentation};
    for width in [120, 88, 60] {
        for (active_status, terminal_status) in [
            (ToolCallStatus::Running, ToolCallStatus::Succeeded),
            (ToolCallStatus::Running, ToolCallStatus::Failed),
            (ToolCallStatus::Running, ToolCallStatus::Cancelled),
            (ToolCallStatus::AwaitingApproval, ToolCallStatus::Denied),
        ] {
            let (mut workspace, mut terminal, sequence) = fixture(width, 0);
            let agent = AgentId::new("primary").expect("agent");
            let item = TranscriptItemId::new("tool").expect("item");
            let event = |revision, status| ConversationEventEnvelope {
                sequence: EventSequence::new(sequence + revision + 1),
                event: ConversationEvent::ToolCallChanged {
                    agent_id: agent.clone(),
                    item_id: item.clone(),
                    item_revision: revision,
                    call_id: ToolCallId::new("call").expect("call"),
                    label: "read_file".into(),
                    status,
                    presentation: ToolPresentation::default(),
                },
            };
            workspace.emit(vec![
                event(0, ToolCallStatus::Queued),
                event(1, active_status),
            ]);
            workspace.settled_draw(&mut terminal).expect("running tool");
            let old = point(&terminal, "read_file");
            assert!(
                workspace
                    .text_point_at(SurfaceId::Transcript, old, false)
                    .is_some()
            );
            workspace.emit(vec![event(2, terminal_status)]);
            workspace
                .draw(&mut terminal)
                .expect("pending terminal status");
            point(&terminal, "Preparing text");
            assert!(
                workspace
                    .text_point_at(SurfaceId::Transcript, old, false)
                    .is_none()
            );
            let work = workspace.take_preparation().expect("current tool request");
            assert_eq!(work.requests[0].key().revision, 2);
            assert!(workspace.complete_preparation(
                work.token,
                work.requests.iter().map(Request::prepare).collect()
            ));
            workspace
                .draw(&mut terminal)
                .expect("current terminal status");
            point(&terminal, "read_file");
        }
    }
}

/// MD-4/PRE-3/FR-3: preparing a streamed revision retains the actual rows and their measured origin.
#[test]
fn streaming_preparation_keeps_the_last_painted_rows_and_geometry() {
    for width in [120, 88, 60] {
        for source in ["Readable text 中文", "**Readable text** 中文"] {
            let (mut workspace, mut terminal, sequence) = fixture(width, 0);
            let agent = AgentId::new("primary").expect("agent");
            let item = TranscriptItemId::new("stream").expect("item");
            workspace.emit(vec![
                ConversationEventEnvelope {
                    sequence: EventSequence::new(sequence + 1),
                    event: ConversationEvent::TranscriptItemStarted {
                        agent_id: agent.clone(),
                        item_id: item.clone(),
                        role: TranscriptRole::Assistant,
                    },
                },
                ConversationEventEnvelope {
                    sequence: EventSequence::new(sequence + 2),
                    event: ConversationEvent::TranscriptDelta {
                        agent_id: agent.clone(),
                        item_id: item.clone(),
                        item_revision: 1,
                        text: source.into(),
                    },
                },
            ]);
            workspace
                .settled_draw(&mut terminal)
                .expect("first content");
            let before = terminal.backend().buffer().clone();
            let viewport = workspace.surfaces.viewport(SurfaceId::Transcript);
            workspace.emit(vec![ConversationEventEnvelope {
                sequence: EventSequence::new(sequence + 3),
                event: ConversationEvent::TranscriptDelta {
                    agent_id: agent,
                    item_id: item,
                    item_revision: 2,
                    text: " continuing with more text".repeat(12),
                },
            }]);
            workspace.draw(&mut terminal).expect("pending update");
            assert_eq!(
                crate::test_support::snapshot_text(terminal.backend().buffer(), before.area),
                crate::test_support::snapshot_text(&before, before.area),
                "{source:?} at {width}: pending preparation erased visible text"
            );
            assert_eq!(workspace.surfaces.viewport(SurfaceId::Transcript), viewport);
            let viewport = viewport.expect("painted viewport");
            let (_, key, _, _) = workspace
                .metrics
                .painted_entry(
                    SurfaceId::Transcript,
                    &AgentId::new("primary").expect("agent"),
                    viewport.content_width,
                    0,
                )
                .expect("painted source identity");
            assert_eq!(
                key.revision, 1,
                "old rows cannot claim the current revision"
            );
            let at = point(&terminal, "Readable text");
            let (_, selected, _) = workspace
                .text_point_at(SurfaceId::Transcript, at, false)
                .expect("retained painted text");
            assert_eq!(selected.offset(), 0);
            let work = workspace
                .take_preparation()
                .expect("new revision is still requested");
            assert_eq!(work.requests[0].key().revision, 2);
            assert!(workspace.complete_preparation(
                work.token,
                work.requests.iter().map(Request::prepare).collect()
            ));
            workspace.draw(&mut terminal).expect("new content");
            point(&terminal, "continuing with more text");
        }
    }
}

/// FR-3/SEL-2/PRE-4: release copies the retained painted fragment immediately, even while the
/// current semantic revision is still waiting on preparation. Copy does not replace that work.
#[test]
fn pending_stream_copy_captures_painted_fragments_without_waiting_for_new_source() {
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal, sequence) = fixture(width, 1);
        workspace
            .settled_draw(&mut terminal)
            .expect("first painted source");
        workspace.emit(vec![ConversationEventEnvelope {
            sequence: EventSequence::new(sequence + 1),
            event: ConversationEvent::TranscriptDelta {
                agent_id: AgentId::new("primary").expect("agent"),
                item_id: TranscriptItemId::new("item-0").expect("item"),
                item_revision: 2,
                text: " a longer continuation".repeat(20),
            },
        }]);
        workspace
            .draw(&mut terminal)
            .expect("retained source frame");
        let work = workspace.take_preparation().expect("pending new source");
        let start = point(&terminal, "message 000");
        let end = Point {
            x: start.x + "message 000 中文".width() as u16,
            ..start
        };
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), start));
        workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), end));
        let copied = workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), end))
            .copied
            .expect("painted text needs no new preparation");
        assert_eq!(copied.text, "message 000 中文");
        assert!(workspace.owns_preparation(&work.token));
        assert_eq!(workspace.copy_preparation_keys().count(), 0);
        assert!(workspace.complete_preparation(
            work.token,
            work.requests.iter().map(Request::prepare).collect()
        ));
        assert_eq!(
            workspace.copy_selection().expect("selection retained").text,
            copied.text
        );
    }
}

/// PRE-4/SEL-2: an empty painted member is a captured absence, not a request to substitute newer
/// unpainted text. A new explicit Copy after painting includes the newly visible member.
#[test]
fn empty_painted_fragments_do_not_copy_unseen_text_or_wait_for_it() {
    let (mut workspace, mut terminal, mut sequence) = fixture(88, 0);
    let agent = AgentId::new("primary").expect("agent");
    for (index, text) in ["Alpha", "", "Omega"].into_iter().enumerate() {
        let item = TranscriptItemId::new(format!("part-{index}")).expect("item");
        for event in [
            ConversationEvent::TranscriptItemStarted {
                agent_id: agent.clone(),
                item_id: item.clone(),
                role: TranscriptRole::Assistant,
            },
            ConversationEvent::TranscriptDelta {
                agent_id: agent.clone(),
                item_id: item,
                item_revision: 1,
                text: text.into(),
            },
        ] {
            sequence += 1;
            workspace.emit(vec![ConversationEventEnvelope {
                sequence: EventSequence::new(sequence),
                event,
            }]);
        }
    }
    workspace
        .settled_draw(&mut terminal)
        .expect("painted empty member");
    workspace.emit(vec![ConversationEventEnvelope {
        sequence: EventSequence::new(sequence + 1),
        event: ConversationEvent::TranscriptDelta {
            agent_id: agent,
            item_id: TranscriptItemId::new("part-1").expect("middle"),
            item_revision: 2,
            text: "new middle".into(),
        },
    }]);
    let first = point(&terminal, "Alpha");
    let last = point(&terminal, "Omega");
    let end = Point {
        x: last.x + 5,
        ..last
    };
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), first));
    workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), end));
    assert_eq!(
        workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), end))
            .copied
            .expect("painted selection is complete")
            .text,
        "Alpha\n\nOmega"
    );
    assert_eq!(workspace.copy_preparation_keys().count(), 0);
    workspace
        .settled_draw(&mut terminal)
        .expect("paint new middle");
    assert_eq!(
        workspace
            .handle(&ctrl('y'))
            .copied
            .expect("new explicit copy")
            .text,
        "Alpha\n\nnew middle\n\nOmega"
    );
}

/// PRE-1/PRE-3/MD-4: a cold frame declares reached work without preparing or cloning rich history.
#[test]
fn cold_preparation_is_deferred_and_hidden_rich_history_is_not_queued() {
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal, _) = fixture(width, 5000);
        let work = workspace
            .draw(&mut terminal)
            .expect("pending frame")
            .expect("frame");
        assert_eq!(
            work.entries_wrapped, 0,
            "unknown rich heights are explicit estimates"
        );
        assert_eq!(
            workspace.metrics.text_layouts(),
            0,
            "draw ran no preparation"
        );
        assert!(work.lines_built <= 24);
        assert!(workspace.metrics.preparation_needed().len() < 16);
        let pending = workspace.take_preparation().expect("visible request");
        assert_ne!(
            pending.requests[0].key().item.as_str(),
            "item-0",
            "the tail, not hidden history, is prepared first"
        );
        assert!(
            workspace.take_preparation().is_none(),
            "one owned request, no cloned history queue"
        );
        workspace.handle(&ctrl('p'));
        workspace
            .draw(&mut terminal)
            .expect("overlay while preparing");
        assert!(workspace.surfaces.get(SurfaceId::CommandPalette).is_some());
        assert_eq!(workspace.metrics.text_layouts(), 0);
        assert_eq!(workspace.handle(&ctrl('d')).flow, Flow::Continue);
        assert_eq!(workspace.handle(&ctrl('d')).flow, Flow::Quit);
    }
}

/// PRE-3: semantic IDs and local sequence numbers can repeat, but workspace generations cannot.
#[test]
fn preparation_cannot_cross_workspace_generations_or_admit_mismatched_keys() {
    for axis in 0..7 {
        let (mut old, mut old_terminal, _) = fixture(88, 1);
        let (mut current, mut terminal, _) = fixture(88, 1);
        old.draw(&mut old_terminal).expect("old pending");
        current.draw(&mut terminal).expect("new pending");
        let old_work = old.take_preparation().expect("old work");
        let work = current.take_preparation().expect("new work");
        assert!(!current.complete_preparation(
            old_work.token,
            old_work.requests.iter().map(Request::prepare).collect()
        ));
        assert!(current.owns_preparation(&work.token));
        assert_eq!(current.metrics.text_layouts(), 0);
        let mut item = current
            .state
            .primary_agent()
            .expect("agent")
            .entries()
            .next()
            .expect("entry")
            .clone();
        let key = work.requests[0].key();
        let mut agent = key.agent.clone();
        let mut width = key.width;
        let mut open = key.open;
        let mut math = key.math;
        match axis {
            0 => agent = AgentId::new("other-agent").expect("agent"),
            1 | 2 => {
                if let crate::TranscriptEntryView::Text(text) = &mut item {
                    if axis == 1 {
                        text.id = TranscriptItemId::new("other-item").expect("item");
                    } else {
                        text.revision += 1;
                    }
                }
            }
            3 => width += 1,
            4 => open = !open,
            5 => math = crate::math::MathPresentation::Native,
            6 => {}
            _ => unreachable!(),
        }
        let prepared = Request::new(agent, item, width, open)
            .with_math(math)
            .prepare();
        assert_eq!(
            current.complete_preparation(work.token, vec![prepared]),
            axis == 6,
            "key axis {axis}"
        );
        current.draw(&mut terminal).expect("admission frame");
        if axis == 6 {
            point(&terminal, "message 000");
        } else {
            point(&terminal, "Text unavailable");
        }
    }
}

/// PRE-3/FR-3: a completion may update retained data, never the text hit map before its frame.
#[test]
fn prepared_text_is_not_selectable_until_the_result_has_been_painted() {
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal, _) = fixture(width, 1);
        workspace.draw(&mut terminal).expect("pending frame");
        let at = point(&terminal, "Preparing text");
        assert!(
            workspace
                .text_point_at(SurfaceId::Transcript, at, false)
                .is_none()
        );
        let work = workspace.take_preparation().expect("work");
        assert!(workspace.complete_preparation(
            work.token,
            work.requests.iter().map(Request::prepare).collect()
        ));
        assert!(
            workspace
                .text_point_at(SurfaceId::Transcript, at, false)
                .is_none(),
            "a ready cache is not a painted frame"
        );
        workspace.draw(&mut terminal).expect("ready frame");
        let at = point(&terminal, "message 000");
        let (_, selected, _) = workspace
            .text_point_at(SurfaceId::Transcript, at, false)
            .expect("painted text");
        assert_eq!(selected.item.as_str(), "item-0");
        assert_eq!(selected.offset(), 0);
        let before = workspace.metrics.text_layouts();
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
        let end = Point { x: at.x + 7, ..at };
        workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), end));
        assert_eq!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), end))
                .copied
                .expect("copy")
                .text,
            "message"
        );
        assert_eq!(
            workspace.metrics.text_layouts(),
            before,
            "neither hit testing nor copy reparses"
        );
    }
}

/// PRE-3: resize and source replacement retire old requests, and failure cannot create a retry spin.
#[test]
fn superseded_preparation_is_ignored_and_failure_is_local_without_an_idle_retry() {
    let (mut workspace, mut terminal, sequence) = fixture(120, 1);
    workspace.draw(&mut terminal).expect("pending");
    let old = workspace.take_preparation().expect("old request");
    terminal.backend_mut().resize(60, 24);
    workspace.handle(&Event::Resize(60, 24));
    workspace.draw(&mut terminal).expect("new width");
    let current = workspace.take_preparation().expect("new width request");
    assert!(!workspace.complete_preparation(
        old.token,
        old.requests.iter().map(Request::prepare).collect()
    ));
    workspace.emit(vec![ConversationEventEnvelope {
        sequence: EventSequence::new(sequence + 1),
        event: ConversationEvent::TranscriptDelta {
            agent_id: AgentId::new("primary").expect("agent"),
            item_id: TranscriptItemId::new("item-0").expect("item"),
            item_revision: 2,
            text: " changed".into(),
        },
    }]);
    assert!(!workspace.complete_preparation(
        current.token,
        current.requests.iter().map(Request::prepare).collect()
    ));
    workspace.draw(&mut terminal).expect("new revision");
    let current = workspace.take_preparation().expect("new revision request");
    workspace.fail_preparation(current.token, Refusal::Unavailable);
    workspace.draw(&mut terminal).expect("local failure");
    point(&terminal, "Text unavailable");
    assert!(workspace.take_preparation().is_none());
    assert!(!workspace.needs_draw());
    workspace
        .state
        .focus_surface(&workspace.surfaces, SurfaceId::Transcript);
    workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)));
    assert_eq!(
        workspace
            .handle(&ctrl('y'))
            .copied
            .expect("source remains available")
            .text,
        "**message 000** 中文 changed"
    );
}

pub(super) fn select_unprepared_history(width: u16) -> (Workspace, Terminal<TestBackend>, u64) {
    let (mut workspace, mut terminal, sequence) = fixture(width, 200);
    workspace.settled_draw(&mut terminal).expect("tail");
    workspace.state.scroll_conversation_by(
        &workspace.surfaces,
        &workspace.metrics,
        SurfaceId::Transcript,
        crate::ScrollDirection::Up,
        usize::MAX,
    );
    workspace.settled_draw(&mut terminal).expect("first entry");
    let first = point(&terminal, "message 000");
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), first));
    workspace.state.scroll_conversation_by(
        &workspace.surfaces,
        &workspace.metrics,
        SurfaceId::Transcript,
        crate::ScrollDirection::Down,
        usize::MAX,
    );
    workspace.settled_draw(&mut terminal).expect("last entry");
    let last = point(&terminal, "message 199");
    let end = Point {
        x: last.x + "message 199 中文".width() as u16,
        ..last
    };
    workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), end));
    assert!(
        workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), end))
            .copied
            .is_none(),
        "unprepared middle entries cannot copy yet"
    );
    assert_eq!(
        workspace
            .state
            .selection()
            .expect("selection retained")
            .bounds(),
        (0, 199)
    );
    assert_eq!(
        workspace.state.copy_note(SurfaceId::Transcript),
        Some(CopyNote::Preparing)
    );
    (workspace, terminal, sequence)
}

/// SEL-1/SEL-2/PRE-3: an off-screen middle is prepared on demand; release preserves the range and
/// the exact plain-text copy is emitted once, without source parsing in input or frame handling.
#[test]
fn selected_text_waits_for_missing_preparation_and_emits_one_complete_copy() {
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal, _) = select_unprepared_history(width);
        workspace.handle(&ctrl('p'));
        workspace
            .draw(&mut terminal)
            .expect("overlay during copy preparation");
        assert!(workspace.surfaces.get(SurfaceId::CommandPalette).is_some());
        assert!(workspace.take_copy().is_none());
        workspace
            .settled_draw(&mut terminal)
            .expect("copy preparation completes");
        let copy = workspace.take_copy().expect("complete copy");
        let expected = (0..200)
            .map(|index| format!("message {index:03} 中文"))
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(copy.text, expected);
        assert_eq!(copy.entries, 200);
        assert!(
            workspace.take_copy().is_none(),
            "auto-copy happens only once"
        );
        assert_eq!(
            workspace.copy_selection().expect("retained copy").text,
            expected
        );
    }
}

/// SEL-1/PRE-3: changing a consumed middle entry or clearing the selection cancels pending copy;
/// endpoint-only revision validation would leak stale text here.
#[test]
fn pending_copy_cannot_outlive_selected_source_changes_or_cancellation() {
    for cancel in [false, true] {
        let (mut workspace, mut terminal, sequence) = select_unprepared_history(88);
        let work = workspace.take_preparation().expect("missing selected text");
        if cancel {
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        } else {
            workspace.emit(vec![ConversationEventEnvelope {
                sequence: EventSequence::new(sequence + 1),
                event: ConversationEvent::TranscriptDelta {
                    agent_id: AgentId::new("primary").expect("agent"),
                    item_id: TranscriptItemId::new("item-10").expect("middle item"),
                    item_revision: 2,
                    text: " changed".into(),
                },
            }]);
            assert_eq!(
                workspace.state.copy_note(SurfaceId::Transcript),
                Some(CopyNote::Changed)
            );
        }
        workspace.complete_preparation(
            work.token,
            work.requests.iter().map(Request::prepare).collect(),
        );
        workspace
            .settled_draw(&mut terminal)
            .expect("settled after cancellation");
        assert!(
            workspace.take_copy().is_none(),
            "late work must not send a superseded copy"
        );
    }
}

/// PRE-1/PRE-3: pending literal height cannot allocate one placeholder per unadmitted source row.
/// Both a newline-dense entry and one exceeding snapshot admission remain bounded and copyable.
#[test]
fn oversized_literal_pending_frame_is_bounded_before_worker_admission() {
    for source in ["\n".repeat(64 * 1024), "x".repeat(2 * 1024 * 1024)] {
        let (mut workspace, mut terminal, sequence) = fixture(88, 0);
        let agent = AgentId::new("primary").expect("agent");
        let item = TranscriptItemId::new("large-literal").expect("item");
        workspace.emit(vec![
            ConversationEventEnvelope {
                sequence: EventSequence::new(sequence + 1),
                event: ConversationEvent::TranscriptItemStarted {
                    agent_id: agent.clone(),
                    item_id: item.clone(),
                    role: TranscriptRole::User,
                },
            },
            ConversationEventEnvelope {
                sequence: EventSequence::new(sequence + 2),
                event: ConversationEvent::TranscriptDelta {
                    agent_id: agent,
                    item_id: item,
                    item_revision: 1,
                    text: source.clone(),
                },
            },
        ]);
        let frame = workspace
            .draw(&mut terminal)
            .expect("pending")
            .expect("frame");
        assert!(
            frame.lines_built <= 2,
            "pending source escaped the prepared-row bound"
        );
        workspace.settled_draw(&mut terminal).expect("refusal");
        point(&terminal, "Text preparation limit");
        assert!(workspace.take_preparation().is_none());
        workspace
            .state
            .focus_surface(&workspace.surfaces, SurfaceId::Transcript);
        workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)));
        assert_eq!(
            workspace
                .handle(&ctrl('y'))
                .copied
                .expect("retained source")
                .text,
            source
        );
    }
}
