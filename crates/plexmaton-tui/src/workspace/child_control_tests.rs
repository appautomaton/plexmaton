use super::*;
use crate::{
    ChildControl, ChildControlRefusal, ChildControlSnapshot, Direction, KeyboardFocus, Outcome,
    Point, SubmissionKind, SurfaceId, test_support::Conversation,
};
use plexmaton_core::{AgentStatus, ConversationEvent};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
};

fn id(value: &str) -> AgentId {
    AgentId::new(value).expect("fixture identity")
}

fn snapshot(revision: u64, control: ChildControl) -> ChildControlSnapshot {
    ChildControlSnapshot { revision, control }
}

fn fixture(width: u16, status: AgentStatus) -> (Workspace, Terminal<TestBackend>) {
    let child = id("agent-b");
    let mut conversation = Conversation::canonical();
    conversation.extend(12).extend_agent(&child, 12);
    conversation.emit(ConversationEvent::AgentStatusChanged {
        agent_id: child.clone(),
        status,
    });
    let mut workspace = Workspace::default();
    workspace.emit(conversation.drain());
    workspace.return_input(id("agent-a"), "primary draft".into());
    workspace.state.select_agent(&child).expect("known child");
    let mut terminal = Terminal::new(TestBackend::new(width, 36)).expect("fixture terminal");
    draw(&mut workspace, &mut terminal);
    (workspace, terminal)
}

fn draw(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) {
    workspace.settled_draw(terminal).expect("fixture draw");
}

fn focus(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>, target: SurfaceId) {
    workspace.state.focus_surface(&workspace.surfaces, target);
    draw(workspace, terminal);
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn screen(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

/// CCV-1: only a known child accepts monotonic, non-conflicting control snapshots.
#[test]
fn ccv_1_snapshots_refuse_wrong_targets_stale_and_conflicting_revisions() {
    let (mut workspace, mut terminal) = fixture(88, AgentStatus::Idle);
    let child = id("agent-b");
    let main = snapshot(1, ChildControl::Main);
    assert_eq!(
        workspace.set_child_control(&id("agent-a"), main),
        Err(ChildControlRefusal::Primary)
    );
    assert!(matches!(
        workspace.set_child_control(&id("missing"), main),
        Err(ChildControlRefusal::UnknownChild(_))
    ));
    assert_eq!(
        workspace.state.agent(&child).expect("child").control(),
        None
    );
    assert_eq!(workspace.set_child_control(&child, main), Ok(true));
    draw(&mut workspace, &mut terminal);
    let revision = workspace.state.revision();
    assert_eq!(workspace.set_child_control(&child, main), Ok(false));
    assert_eq!(
        workspace.set_child_control(&child, snapshot(0, ChildControl::User)),
        Err(ChildControlRefusal::Stale)
    );
    assert_eq!(
        workspace.set_child_control(&child, snapshot(1, ChildControl::User)),
        Err(ChildControlRefusal::Conflict)
    );
    assert_eq!(
        workspace.set_child_control(&child, snapshot(2, ChildControl::Main)),
        Ok(false)
    );
    assert_eq!(
        workspace.state.revision(),
        revision,
        "no visible change costs no frame"
    );
    assert_eq!(
        workspace.state.agent(&child).expect("child").control(),
        Some(snapshot(2, ChildControl::Main))
    );
}

/// CCV-2/COM-4: activity never opens an unknown/Main/pending child's input at any layout class.
#[test]
fn ccv_2_non_user_children_have_no_input_cursor_or_submission() {
    for width in [120, 88, 60] {
        for status in [
            AgentStatus::Running,
            AgentStatus::Idle,
            AgentStatus::Completed,
        ] {
            for control in [
                None,
                Some(ChildControl::Main),
                Some(ChildControl::HandoffPending),
            ] {
                let (mut workspace, mut terminal) = fixture(width, status);
                let child = id("agent-b");
                if let Some(control) = control {
                    workspace
                        .set_child_control(&child, snapshot(1, control))
                        .expect("snapshot");
                }
                workspace.return_input(child.clone(), "retained child draft".into());
                draw(&mut workspace, &mut terminal);
                let composer = workspace
                    .surfaces
                    .get(SurfaceId::Composer)
                    .expect("composer")
                    .bounds;
                focus(&mut workspace, &mut terminal, SurfaceId::Inspector);
                assert_eq!(
                    workspace.state.keyboard_focus(&workspace.surfaces),
                    KeyboardFocus::Navigation
                );
                assert_eq!(workspace.state.text_target(&workspace.surfaces), None);
                assert!(workspace.state.steer_input(&workspace.surfaces).is_none());
                assert!(!terminal.backend().cursor_visible());
                assert_eq!(
                    workspace
                        .surfaces
                        .get(SurfaceId::Composer)
                        .expect("composer")
                        .bounds,
                    composer
                );
                assert_eq!(
                    workspace.handle(&Event::Paste("must not land".into())),
                    Outcome::default()
                );
                assert!(
                    workspace
                        .handle(&key(KeyCode::Enter, KeyModifiers::NONE))
                        .submitted
                        .is_none()
                );
                assert_eq!(workspace.state.draft(&child).text(), "retained child draft");
                assert_eq!(workspace.state.composer().text(), "primary draft");
            }
        }
    }
}

/// CCV-3/INS-6: fixed controller/capability chrome cannot become selectable transcript source.
#[test]
fn ccv_3_control_chrome_stays_outside_the_transcript_at_three_widths() {
    for width in [120, 88, 60] {
        for control in [
            ChildControl::Main,
            ChildControl::HandoffPending,
            ChildControl::User,
        ] {
            let (mut workspace, mut terminal) = fixture(width, AgentStatus::Running);
            focus(&mut workspace, &mut terminal, SurfaceId::Composer);
            workspace
                .set_child_control(&id("agent-b"), snapshot(1, control))
                .expect("snapshot");
            draw(&mut workspace, &mut terminal);
            let frame = screen(&terminal);
            assert!(
                !frame.contains("^C Stop"),
                "a child shortcut must not claim the primary's keyboard"
            );
            // Who drives is a glyph and a word; the capability boundary is one glyph, because it
            // is the same constant for every child and does not earn a sentence in every frame.
            let controller = if control == ChildControl::User {
                "\u{f0004} User"
            } else {
                "\u{f06a9} Main"
            };
            assert!(
                frame.contains(&format!("{controller}  {}", '\u{f099d}')),
                "{width}: {frame}"
            );
            assert!(frame.contains(if control == ChildControl::HandoffPending {
                "Handoff pending"
            } else {
                "Running"
            }));
            let surface = workspace
                .surfaces
                .get(SurfaceId::Inspector)
                .expect("inspector");
            let footer = Point {
                x: surface.bounds.x + 3,
                y: surface.bounds.bottom() - 2,
            };
            assert!(
                workspace
                    .entry_target_at(SurfaceId::Inspector, footer)
                    .is_none()
            );
        }
    }
}

/// CCV-4: acknowledgment preserves focus, both drafts, selection and parked reader anchors.
#[test]
fn ccv_4_acknowledgment_is_passive_and_preserves_reading_state() {
    for focused in [SurfaceId::Composer, SurfaceId::Inspector] {
        let (mut workspace, mut terminal) = fixture(120, AgentStatus::Idle);
        let child = id("agent-b");
        workspace
            .set_child_control(&child, snapshot(1, ChildControl::Main))
            .expect("snapshot");
        workspace.return_input(child.clone(), "child draft".into());
        focus(&mut workspace, &mut terminal, SurfaceId::Inspector);
        workspace
            .state
            .select(&workspace.surfaces, Direction::Backward);
        draw(&mut workspace, &mut terminal);
        let bounds = workspace
            .surfaces
            .get(SurfaceId::Inspector)
            .expect("inspector")
            .bounds;
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: bounds.x + 3,
            row: bounds.y + 4,
            modifiers: KeyModifiers::NONE,
        }));
        draw(&mut workspace, &mut terminal);
        focus(&mut workspace, &mut terminal, focused);
        let anchor = workspace.state.conversation_position(&child).cloned();
        let selection = workspace.state.selection().cloned();
        let history = workspace
            .state
            .agent(&child)
            .expect("child")
            .entries()
            .cloned()
            .collect::<Vec<_>>();
        for (revision, control) in [(2, ChildControl::HandoffPending), (3, ChildControl::User)] {
            workspace
                .set_child_control(&child, snapshot(revision, control))
                .expect("snapshot");
            draw(&mut workspace, &mut terminal);
            assert_eq!(workspace.state.focused(&workspace.surfaces), Some(focused));
            assert_eq!(
                workspace.state.conversation_position(&child),
                anchor.as_ref()
            );
            assert_eq!(workspace.state.selection(), selection.as_ref());
            assert_eq!(workspace.state.draft(&child).text(), "child draft");
            assert_eq!(workspace.state.composer().text(), "primary draft");
            assert_eq!(
                workspace
                    .state
                    .agent(&child)
                    .expect("child")
                    .entries()
                    .cloned()
                    .collect::<Vec<_>>(),
                history
            );
        }
        assert_eq!(
            workspace.state.text_target(&workspace.surfaces),
            Some(if focused == SurfaceId::Inspector {
                child
            } else {
                id("agent-a")
            })
        );
    }
}

/// CCV-4: Ctrl-C on a non-editable child addresses Stop without deleting a hidden draft or handing off.
#[test]
fn ccv_4_interrupt_preserves_hidden_input_and_control() {
    let (mut workspace, mut terminal) = fixture(88, AgentStatus::Running);
    let child = id("agent-b");
    workspace
        .set_child_control(&child, snapshot(1, ChildControl::Main))
        .expect("snapshot");
    workspace.return_input(child.clone(), "hidden draft".into());
    focus(&mut workspace, &mut terminal, SurfaceId::Inspector);
    assert!(screen(&terminal).contains("^C Stop"));
    let outcome = workspace.handle(&key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(outcome.interrupted, Some(child.clone()));
    assert_eq!(workspace.state.draft(&child).text(), "hidden draft");
    assert_eq!(
        workspace.state.agent(&child).expect("child").control(),
        Some(snapshot(1, ChildControl::Main))
    );
    assert_eq!(
        workspace.state.agent(&child).expect("child").status,
        AgentStatus::Running
    );
}

/// CCV-2/INS-3: reopening and responsive geometry preserve control and hidden drafts.
#[test]
fn ccv_2_control_survives_dismissal_reopen_and_resize() {
    let (mut workspace, mut terminal) = fixture(120, AgentStatus::Idle);
    let child = id("agent-b");
    let main = snapshot(1, ChildControl::Main);
    workspace.set_child_control(&child, main).expect("snapshot");
    workspace.return_input(child.clone(), "retained".into());
    focus(&mut workspace, &mut terminal, SurfaceId::Inspector);
    workspace.handle(&key(KeyCode::Esc, KeyModifiers::NONE));
    draw(&mut workspace, &mut terminal);
    assert!(workspace.state.inspector().is_none());
    workspace.state.select_agent(&child).expect("known child");
    draw(&mut workspace, &mut terminal);
    focus(&mut workspace, &mut terminal, SurfaceId::Inspector);
    for width in [88, 60, 120] {
        terminal.backend_mut().resize(width, 36);
        workspace.handle(&Event::Resize(width, 36));
        draw(&mut workspace, &mut terminal);
        assert_eq!(
            workspace.state.agent(&child).expect("child").control(),
            Some(main)
        );
        assert_eq!(workspace.state.draft(&child).text(), "retained");
        assert_eq!(
            workspace.state.keyboard_focus(&workspace.surfaces),
            KeyboardFocus::Navigation
        );
    }
}

/// CCV-2/COM-4: an acknowledged child routes idle input to a turn and busy input to a step.
#[test]
fn ccv_2_user_child_input_uses_its_lifecycle_without_primary_commands() {
    for (status, kind) in [
        (AgentStatus::Idle, SubmissionKind::Message),
        (AgentStatus::Running, SubmissionKind::Steering),
    ] {
        let (mut workspace, mut terminal) = fixture(88, status);
        let child = id("agent-b");
        workspace
            .set_child_control(&child, snapshot(1, ChildControl::User))
            .expect("snapshot");
        focus(&mut workspace, &mut terminal, SurfaceId::Inspector);
        workspace.handle(&Event::Paste("/model child input".into()));
        draw(&mut workspace, &mut terminal);
        let outcome = workspace.handle(&key(KeyCode::Enter, KeyModifiers::NONE));
        let submitted = outcome.submitted.expect("user-controlled input");
        assert_eq!(submitted.to, child);
        assert_eq!(submitted.kind, kind);
        assert_eq!(submitted.text, "/model child input");
        assert!(outcome.command.is_none() && outcome.model.is_none() && outcome.effort.is_none());
        assert_eq!(workspace.state.composer().text(), "primary draft");
    }
}

/// CCV-2/INS-5/INS-7: collapse reflects an actual input, including shelf/maximized thresholds.
#[test]
fn ccv_2_primary_collapse_matches_visible_child_input_across_short_heights() {
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal) = fixture(width, AgentStatus::Idle);
        workspace
            .set_child_control(&id("agent-b"), snapshot(1, ChildControl::User))
            .expect("snapshot");
        focus(&mut workspace, &mut terminal, SurfaceId::Inspector);
        for height in 12..45 {
            terminal.backend_mut().resize(width, height);
            workspace.handle(&Event::Resize(width, height));
            draw(&mut workspace, &mut terminal);
            let has_input = workspace.state.steer_input(&workspace.surfaces).is_some();
            let composer = workspace
                .surfaces
                .get(SurfaceId::Composer)
                .expect("composer")
                .bounds;
            assert_eq!(
                composer.height,
                if has_input { 2 } else { 3 },
                "{width}x{height}"
            );
        }
    }
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn select_child_draft(
    workspace: &mut Workspace,
    terminal: &mut Terminal<TestBackend>,
    release: bool,
) -> (u16, u16) {
    workspace.return_input(id("agent-b"), "abcdef".into());
    draw(workspace, terminal);
    let (split, _) = workspace
        .state
        .steer_input(&workspace.surfaces)
        .expect("child input");
    let (x, y) = (split.input.x + 1, split.input.y + 1);
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    draw(workspace, terminal);
    workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), x + 3, y));
    draw(workspace, terminal);
    if release {
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), x + 3, y))
                .copied
                .is_some()
        );
        draw(workspace, terminal);
    }
    assert_eq!(
        workspace.state.draft(&id("agent-b")).selected_text(),
        Some("abc")
    );
    (x + 3, y)
}

/// CCV-4/SEL-4: control loss settles capture without delivering hidden selected source.
#[test]
fn ccv_4_control_loss_settles_input_drag_without_copy_or_hidden_escape() {
    let (mut workspace, mut terminal) = fixture(120, AgentStatus::Idle);
    let child = id("agent-b");
    workspace
        .set_child_control(&child, snapshot(1, ChildControl::User))
        .expect("snapshot");
    focus(&mut workspace, &mut terminal, SurfaceId::Inspector);
    let (x, y) = select_child_draft(&mut workspace, &mut terminal, false);
    assert_eq!(workspace.router.capture(), Some(SurfaceId::Inspector));
    workspace
        .set_child_control(&child, snapshot(2, ChildControl::Main))
        .expect("snapshot");
    draw(&mut workspace, &mut terminal);
    assert_eq!(workspace.router.capture(), None);
    assert!(!workspace.state.draft(&child).is_dragging());
    assert_eq!(workspace.state.draft(&child).selected_text(), Some("abc"));
    assert!(
        workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), x, y))
            .copied
            .is_none()
    );
    workspace.handle(&key(KeyCode::Esc, KeyModifiers::NONE));
    draw(&mut workspace, &mut terminal);
    assert!(
        workspace.state.inspector().is_none(),
        "Escape must not act on an invisible selection"
    );
    assert_eq!(workspace.state.draft(&child).text(), "abcdef");
    assert_eq!(workspace.state.draft(&child).selected_text(), Some("abc"));
}

/// CCV-4/INS-7: a captured release after shrinking away input retains its range without copying.
#[test]
fn ccv_4_hidden_input_release_settles_and_escape_closes_the_window() {
    for released in [false, true] {
        let (mut workspace, mut terminal) = fixture(120, AgentStatus::Idle);
        let child = id("agent-b");
        workspace
            .set_child_control(&child, snapshot(1, ChildControl::User))
            .expect("snapshot");
        focus(&mut workspace, &mut terminal, SurfaceId::Inspector);
        let (x, y) = select_child_draft(&mut workspace, &mut terminal, released);
        for _ in 0..36 {
            workspace.handle(&key(
                KeyCode::Up,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ));
            draw(&mut workspace, &mut terminal);
        }
        assert!(workspace.state.steer_input(&workspace.surfaces).is_none());
        if !released {
            assert!(
                workspace
                    .handle(&mouse(MouseEventKind::Up(MouseButton::Left), x, y))
                    .copied
                    .is_none()
            );
        }
        assert!(!workspace.state.draft(&child).is_dragging());
        workspace.handle(&key(KeyCode::Esc, KeyModifiers::NONE));
        draw(&mut workspace, &mut terminal);
        assert!(workspace.state.inspector().is_none());
        assert_eq!(workspace.state.draft(&child).selected_text(), Some("abc"));
        assert_eq!(workspace.state.draft(&child).text(), "abcdef");
    }
}
