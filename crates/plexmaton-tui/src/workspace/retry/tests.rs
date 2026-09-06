use super::*;
use crate::{SkillChoice, SkillChoiceSource};
use plexmaton_core::{
    AgentStatus, ConversationEvent, EventSequence, TranscriptItemId, TranscriptRole, TurnId,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn mouse(kind: MouseEventKind, point: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: point.x,
        row: point.y,
        modifiers: KeyModifiers::NONE,
    })
}

fn fixture(width: u16) -> (Workspace, Terminal<TestBackend>, Point) {
    let mut workspace = Workspace::default();
    let agent = AgentId::new("primary").expect("agent");
    let question = TranscriptItemId::new("question").expect("id");
    let error = TranscriptItemId::new("error").expect("id");
    let events = vec![
        ConversationEvent::AgentCreated {
            agent_id: agent.clone(),
            label: "Plexmaton".into(),
            status: AgentStatus::Idle,
        },
        ConversationEvent::TranscriptItemStarted {
            agent_id: agent.clone(),
            item_id: question.clone(),
            role: TranscriptRole::User,
        },
        ConversationEvent::TranscriptDelta {
            agent_id: agent.clone(),
            item_id: question.clone(),
            item_revision: 1,
            text: "Original question".into(),
        },
        ConversationEvent::RuntimeError {
            agent_id: agent,
            item_id: error.clone(),
            message: "Request was rate limited.".into(),
        },
    ];
    workspace.emit(
        events
            .into_iter()
            .enumerate()
            .map(|(i, event)| ConversationEventEnvelope {
                sequence: EventSequence::new(i as u64 + 1),
                event,
            })
            .collect(),
    );
    workspace.set_retry_actions(Some(RetryActions {
        target: RetryTarget {
            turn_id: TurnId::new("failed-turn").expect("id"),
            revision: 5,
        },
        question_item: question,
        error_item: error,
        skill: None,
    }));
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
    workspace.settled_draw(&mut terminal).expect("draw");
    let buffer = terminal.backend().buffer();
    let point = (0..24)
        .find_map(|y| {
            let row = (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            row.find("[ Retry ]").map(|x| Point { x: x as u16, y })
        })
        .expect("visible retry controls");
    (workspace, terminal, point)
}

/// TR-1: feedback height follows the same wrapped rows as paint. The 48-column workspace is
/// real; the narrower admitted content width additionally exercises wrapping of the control line.
#[test]
fn retry_feedback_measurement_counts_wrapped_controls() {
    use ratatui::widgets::{Paragraph, Wrap};
    let (workspace, _, _) = fixture(48);
    let agent = workspace.state.primary_agent().expect("primary");
    for (width, footer_rows) in [(28, 3), (46, 2), (58, 2), (86, 2), (118, 2)] {
        let mut metrics = TranscriptMetrics::default();
        for entry in agent.entries() {
            metrics.accept_prepared(
                crate::preparation::Request::new(agent.id.clone(), entry.clone(), width, false)
                    .prepare(),
            );
        }
        let mut without_controls = agent.clone();
        without_controls.retry = None;
        metrics.measure(&without_controls, &workspace.palette, width);
        let body_rows = metrics.total_rows(&agent.id, width);
        metrics.measure(agent, &workspace.palette, width);
        let window = metrics.window(&agent.id, width, 0, u16::MAX);
        let (lines, skip) = metrics.build(
            agent,
            &workspace.palette,
            &window,
            &workspace.state,
            SurfaceId::Transcript,
        );
        assert_eq!(skip, 0);
        let drawn = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .line_count(width);
        assert_eq!(drawn - body_rows, footer_rows, "content width {width}");
        assert_eq!(metrics.total_rows(&agent.id, width), drawn);
    }
}

/// SKP-3/COM-7: a historical numeric binding survives unchanged Edit & retry.
#[test]
fn unchanged_numeric_skill_retry_uses_the_historical_semantic_binding() {
    let (mut workspace, _terminal, _) = fixture(95);
    let question = TranscriptItemId::new("numeric-question").expect("id");
    workspace.emit(vec![
        ConversationEventEnvelope {
            sequence: EventSequence::new(5),
            event: ConversationEvent::TranscriptItemStarted {
                agent_id: AgentId::new("primary").expect("agent"),
                item_id: question.clone(),
                role: TranscriptRole::User,
            },
        },
        ConversationEventEnvelope {
            sequence: EventSequence::new(6),
            event: ConversationEvent::TranscriptDelta {
                agent_id: AgentId::new("primary").expect("agent"),
                item_id: question.clone(),
                item_revision: 1,
                text: "$100 inspect".to_owned(),
            },
        },
    ]);
    workspace.set_retry_actions(Some(RetryActions {
        target: RetryTarget {
            turn_id: TurnId::new("numeric-turn").expect("turn"),
            revision: 6,
        },
        question_item: question,
        error_item: TranscriptItemId::new("error").expect("error"),
        skill: Some("100".to_owned()),
    }));

    workspace.perform_retry_action(RetryAction::EditRetry);
    let retry = workspace
        .handle(&key(KeyCode::Enter))
        .retry
        .expect("edited retry");
    assert_eq!(retry.edited_text.as_deref(), Some("$100 inspect"));
    assert_eq!(retry.skill.as_deref(), Some("100"));
}

/// SKP-3/COM-7: edit-retry owns its selected skill while the displaced draft keeps its own binding.
#[test]
fn retry_edit_submission_and_saved_draft_keep_independent_skill_bindings() {
    let (mut workspace, mut terminal, _) = fixture(95);
    workspace.set_skills(vec![
        SkillChoice {
            name: "review".to_owned(),
            description: "Review".to_owned(),
            source: SkillChoiceSource::ProjectNative,
        },
        SkillChoice {
            name: "100".to_owned(),
            description: "Numeric".to_owned(),
            source: SkillChoiceSource::User,
        },
    ]);
    workspace.handle(&key(KeyCode::Tab));
    workspace.draw(&mut terminal).expect("focus composer");
    workspace.handle(&key(KeyCode::Char('$')));
    workspace.draw(&mut terminal).expect("picker");
    workspace.handle(&key(KeyCode::Enter));
    workspace.draw(&mut terminal).expect("accepted skill");
    workspace.handle(&Event::Paste("saved".to_owned()));

    workspace.perform_retry_action(RetryAction::EditRetry);
    for event in [
        Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)),
    ] {
        workspace.handle(&event);
    }
    workspace.handle(&key(KeyCode::Char('$')));
    workspace.draw(&mut terminal).expect("retry picker");
    workspace.handle(&key(KeyCode::Down));
    workspace.handle(&key(KeyCode::Enter));
    workspace.draw(&mut terminal).expect("retry choice");
    workspace.handle(&Event::Paste("edited".to_owned()));
    let retry = workspace
        .handle(&key(KeyCode::Enter))
        .retry
        .expect("retry submission");
    assert_eq!(retry.edited_text.as_deref(), Some("$100 edited"));
    assert_eq!(retry.skill.as_deref(), Some("100"));

    workspace.complete_retry_edit();
    let saved = workspace
        .handle(&key(KeyCode::Enter))
        .submitted
        .expect("saved draft submission");
    assert_eq!(saved.text, "$review saved");
    assert_eq!(saved.skill.as_deref(), Some("review"));
}

/// TR-1/INV-1: review the actionable failure at each supported composition width.
#[test]
fn retry_frames_keep_actions_with_the_failed_request_at_three_widths() {
    for (width, name) in [(120, "wide"), (95, "medium"), (60, "narrow")] {
        let (_, terminal, _) = fixture(width);
        let buffer = terminal.backend().buffer();
        let frame = crate::test_support::snapshot_text(buffer, buffer.area);
        crate::test_support::assert_frame(&format!("retry-{name}"), &frame);
        assert!(!frame.contains("Notices"));
        assert!(frame.find("Request was rate limited.") < frame.find("[ Retry ]"));
    }
}

/// INV-1/TR-3: real terminal mouse routing and contextual keys emit the same addressed retry.
#[test]
fn retry_click_keyboard_and_drag_cancellation_share_one_action() {
    for width in [120, 95, 60] {
        let (mut workspace, mut terminal, point) = fixture(width);
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
        let moved = Point {
            x: point.x + 1,
            ..point
        };
        workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), moved));
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), point))
                .retry
                .is_none()
        );
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
        let clicked = workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), point))
            .retry
            .expect("click retry");
        assert!(
            workspace.perform_retry_action(RetryAction::Retry).is_none(),
            "duplicate click disabled"
        );
        let (mut keyboard, _, _) = fixture(width);
        assert_eq!(
            keyboard.handle(&key(KeyCode::Char('r'))).retry,
            Some(clicked)
        );
        workspace
            .settled_draw(&mut terminal)
            .expect("remove actions");
    }
}

/// LOOP-6: editing owns a distinct draft mode; Escape restores displaced text without a request.
#[test]
fn edit_retry_keeps_exact_text_until_ack_and_escape_restores_displaced_draft() {
    for width in [120, 95, 60] {
        let (mut workspace, mut terminal, point) = fixture(width);
        workspace.handle(&key(KeyCode::Tab));
        workspace.handle(&Event::Paste("unrelated draft".into()));
        let edit = Point {
            x: point.x + 13,
            ..point
        };
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), edit));
        workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), edit));
        workspace
            .settled_draw(&mut terminal)
            .expect("editing frame");
        assert_eq!(workspace.state.composer().text(), "Original question");
        let submission = workspace.handle(&key(KeyCode::Enter));
        assert!(submission.submitted.is_none());
        assert_eq!(
            submission
                .retry
                .expect("edit submission")
                .edited_text
                .as_deref(),
            Some("Original question")
        );
        assert_eq!(
            workspace.state.composer().text(),
            "Original question",
            "not acknowledged yet"
        );
        workspace.handle(&key(KeyCode::Esc));
        assert_eq!(workspace.state.composer().text(), "unrelated draft");
        assert!(workspace.handle(&key(KeyCode::Enter)).submitted.is_some());
    }
}

/// COM-7: choosing plain Retry cancels edit mode, whose revision belongs to the failed tail.
#[test]
fn plain_retry_leaves_edit_mode_and_restores_the_displaced_draft() {
    let (mut workspace, _, _) = fixture(95);
    workspace.handle(&key(KeyCode::Tab));
    workspace.handle(&Event::Paste("saved draft".into()));
    workspace.perform_retry_action(RetryAction::EditRetry);
    let retry = workspace
        .perform_retry_action(RetryAction::Retry)
        .expect("plain retry");
    assert!(retry.edited_text.is_none());
    assert!(!workspace.state.editing_retry());
    assert_eq!(workspace.state.composer().text(), "saved draft");
}

/// INV-6/COM-6/COM-7: Escape clears editable selection before leaving edited-retry mode.
#[test]
fn escape_clears_retry_input_selection_before_cancelling_the_edit() {
    let (mut workspace, mut terminal, _) = fixture(95);
    workspace.perform_retry_action(RetryAction::EditRetry);
    workspace
        .settled_draw(&mut terminal)
        .expect("editing frame");
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::Composer)
        .expect("composer")
        .bounds;
    let start = Point {
        x: bounds.x + 1,
        y: bounds.y + 1,
    };
    let end = Point {
        x: start.x + 8,
        ..start
    };
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), start));
    workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), end));
    workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), end));
    assert!(workspace.state.composer().selected_text().is_some());
    workspace.handle(&key(KeyCode::Esc));
    assert!(workspace.state.editing_retry());
    assert_eq!(workspace.state.composer().text(), "Original question");
    assert!(workspace.state.composer().selected_text().is_none());
    workspace.handle(&key(KeyCode::Esc));
    assert!(!workspace.state.editing_retry());
}

/// COM-7/SURF-3: replacing semantic history preserves the restored draft's insertion focus.
#[test]
fn acknowledged_edit_retry_restores_the_draft_without_losing_keyboard_focus() {
    let (mut workspace, mut terminal, _) = fixture(95);
    workspace.handle(&key(KeyCode::Tab));
    workspace.handle(&Event::Paste("saved draft".into()));
    workspace.perform_retry_action(RetryAction::EditRetry);
    workspace.complete_retry_edit();
    workspace.replace_projection(vec![ConversationEventEnvelope {
        sequence: EventSequence::new(1),
        event: ConversationEvent::AgentCreated {
            agent_id: AgentId::new("primary").expect("agent"),
            label: "Plexmaton".into(),
            status: AgentStatus::Idle,
        },
    }]);
    workspace
        .settled_draw(&mut terminal)
        .expect("replacement frame");
    assert_eq!(
        workspace.state.focused(workspace.surfaces()),
        Some(SurfaceId::Composer)
    );
    workspace.handle(&Event::Paste(" more".into()));
    assert_eq!(workspace.state.composer().text(), "saved draft more");
}

/// SEL-2/SEL-4/INV-3: hover reveals Copy without reflow/focus/selection; only its click copies.
#[test]
fn message_copy_hover_frames_are_local_and_clicking_body_never_copies() {
    for width in [120, 95, 60] {
        let (mut workspace, mut terminal, _) = fixture(width);
        let row = (0..24)
            .find(|&y| {
                (0..width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
                    .contains("Original question")
            })
            .expect("question");
        let body = Point { x: 8, y: row };
        let focus = workspace.state.focused(workspace.surfaces());
        let wrapped = workspace.metrics().wrapped();
        workspace.handle(&mouse(MouseEventKind::Moved, body));
        workspace.settled_draw(&mut terminal).expect("hover");
        assert_eq!(workspace.state.focused(workspace.surfaces()), focus);
        assert!(workspace.state.selection().is_none());
        assert_eq!(workspace.metrics().wrapped(), wrapped);
        let icon = (0..width)
            .find(|&x| terminal.backend().buffer()[(x, row)].symbol() == "󰆏")
            .expect("copy icon aligned with the message");
        let icon = Point { x: icon, y: row };
        let base = terminal.backend().buffer()[(icon.x, icon.y)].style();
        workspace.handle(&mouse(MouseEventKind::Moved, icon));
        workspace.settled_draw(&mut terminal).expect("icon hover");
        let name = match width {
            120 => "wide",
            95 => "medium",
            _ => "narrow",
        };
        let buffer = terminal.backend().buffer();
        let frame = crate::test_support::snapshot_text(buffer, buffer.area);
        crate::test_support::assert_frame(&format!("message-actions-{name}"), &frame);
        assert_ne!(
            base.fg,
            terminal.backend().buffer()[(icon.x, icon.y)].style().fg
        );
        let revision = workspace.state.revision();
        workspace.handle(&mouse(MouseEventKind::Moved, icon));
        assert_eq!(workspace.state.revision(), revision);
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), icon));
        let copied = workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), icon))
            .copied
            .expect("explicit copy");
        assert_eq!(copied.text, "Original question");
        assert!(workspace.state.selection().is_none());
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), body));
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), body))
                .copied
                .is_none()
        );
        assert!(workspace.state.selection().is_none());
        workspace.handle(&mouse(MouseEventKind::Moved, icon));
        workspace.settled_draw(&mut terminal).expect("icon visible");
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), icon));
        workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), body));
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), icon))
                .copied
                .is_none()
        );
    }
}

/// INV-3: retry highlights only its own label, with no reverse-video selection or remeasurement.
#[test]
fn retry_hover_does_not_reverse_the_button_row_or_interfere_with_selection() {
    use ratatui::style::Modifier;
    let (mut workspace, mut terminal, button) = fixture(95);
    let before = workspace.metrics().wrapped();
    workspace.handle(&mouse(MouseEventKind::Moved, button));
    workspace.settled_draw(&mut terminal).expect("hover retry");
    let cell = terminal.backend().buffer()[(button.x, button.y)].style();
    assert!(!cell.add_modifier.contains(Modifier::REVERSED));
    assert_eq!(workspace.metrics().wrapped(), before);
    assert!(workspace.state.selection().is_none());
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), button));
    workspace.handle(&mouse(MouseEventKind::Moved, button));
    assert!(
        workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), button))
            .retry
            .is_none(),
        "lost-button move disarms Retry"
    );
}

/// SEL-3/SEL-7: Copy is not a selection gesture, including when another message is selected.
#[test]
fn copying_or_cancelling_copy_preserves_an_existing_selection() {
    for cancel in [false, true] {
        let (mut workspace, mut terminal, _) = fixture(95);
        workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)));
        let selected = workspace.state.selection().expect("selected error").clone();
        assert_eq!(selected.bounds(), (1, 1));
        let body_y = (0..24)
            .find(|&y| {
                (0..95)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
                    .contains("Original question")
            })
            .expect("question");
        let body = Point { x: 8, y: body_y };
        workspace.handle(&mouse(MouseEventKind::Moved, body));
        workspace.settled_draw(&mut terminal).expect("hover");
        let x = (0..95)
            .find(|&x| terminal.backend().buffer()[(x, body_y)].symbol() == "󰆏")
            .expect("icon");
        let icon = Point { x, y: body_y };
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), icon));
        if cancel {
            workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), body));
        }
        let copied = workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), icon))
            .copied;
        assert_eq!(copied.is_none(), cancel);
        assert_eq!(workspace.state.selection(), Some(&selected));
    }
}

/// SEL-7: the action's right inset is independent of role gutters and wrapped source spans.
#[test]
fn copy_icons_share_one_right_edge_across_roles_and_wrapped_text() {
    for width in [120, 95, 60] {
        for role in [
            TranscriptRole::User,
            TranscriptRole::Assistant,
            TranscriptRole::Reasoning,
            TranscriptRole::System,
        ] {
            for source in [
                "alignment short",
                "alignment 中文 e\u{301} words followed by several spaces     and more text to wrap across a line",
                "alignment first line\nsecond line with **markup** and more words to occupy the available width",
            ] {
                let (mut workspace, mut terminal, _) = fixture(width);
                let agent_id = AgentId::new("primary").expect("agent");
                let item_id = TranscriptItemId::new("alignment").expect("item");
                workspace.emit(vec![
                    ConversationEventEnvelope {
                        sequence: EventSequence::new(5),
                        event: ConversationEvent::TranscriptItemStarted {
                            agent_id: agent_id.clone(),
                            item_id: item_id.clone(),
                            role,
                        },
                    },
                    ConversationEventEnvelope {
                        sequence: EventSequence::new(6),
                        event: ConversationEvent::TranscriptDelta {
                            agent_id,
                            item_id,
                            item_revision: 1,
                            text: source.into(),
                        },
                    },
                ]);
                workspace.settled_draw(&mut terminal).expect("source frame");
                let row = (0..24)
                    .find(|&y| {
                        (0..width)
                            .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                            .collect::<String>()
                            .contains("alignment")
                    })
                    .expect("visible message");
                workspace.handle(&mouse(MouseEventKind::Moved, Point { x: 8, y: row }));
                workspace.settled_draw(&mut terminal).expect("hover frame");
                let icon = (0..24)
                    .find_map(|y| {
                        (0..width).find(|&x| terminal.backend().buffer()[(x, y)].symbol() == "󰆏")
                    })
                    .expect("copy icon");
                assert_eq!(
                    icon,
                    width - 4,
                    "role {role:?}, width {width}, source {source:?}"
                );
            }
        }
    }
}
