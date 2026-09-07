use super::approval_queue_tests::{agent, allow_button, attention, emit, key, mouse, request};
use super::*;
use crate::{Point, SurfaceId};
use plexmaton_core::{
    AgentStatus, ApprovalDecision, AttentionRequest, ConversationEvent, ToolCallId,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind},
};

fn interaction_fixture(
    width: u16,
    height: u16,
    source: &str,
) -> (Workspace, Terminal<TestBackend>, u64) {
    let mut workspace = Workspace::with_palette(Palette::pastel());
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    let mut next = 1;
    emit(
        &mut workspace,
        &mut next,
        ConversationEvent::AgentCreated {
            agent_id: agent(),
            label: "Plexmaton".into(),
            status: AgentStatus::Waiting,
        },
    );
    let presentation = plexmaton_core::ToolPresentation {
        invocation: Some(plexmaton_core::ToolDetail::Command(Box::new(
            plexmaton_core::CommandInvocation {
                source: source.into(),
                workspace_root: "/workspace".into(),
                timeout_ms: 10_000,
            },
        ))),
        outcome: None,
    };
    for (revision, status) in [
        (0, plexmaton_core::ToolCallStatus::Queued),
        (1, plexmaton_core::ToolCallStatus::AwaitingApproval),
    ] {
        emit(
            &mut workspace,
            &mut next,
            ConversationEvent::ToolCallChanged {
                agent_id: agent(),
                item_id: plexmaton_core::TranscriptItemId::new("tool-0").expect("entry"),
                item_revision: revision,
                call_id: ToolCallId::new("call-0").expect("call"),
                label: "exec_command".into(),
                status,
                presentation: presentation.clone(),
            },
        );
    }
    let mut event = request(0);
    if let ConversationEvent::AttentionRequested {
        request: AttentionRequest::Approval {
            remember, detail, ..
        },
        ..
    } = &mut event
    {
        *detail = format!("Command {source:?}");
        *remember = Some(plexmaton_core::RememberPermissionOffer {
            id: plexmaton_core::PermissionOfferId::new(1),
            label: "same command in this checkout".into(),
            note: None,
            scopes: plexmaton_core::PermissionScopes::SessionAndProject,
        });
    }
    emit(&mut workspace, &mut next, event);
    workspace
        .settled_draw(&mut terminal)
        .expect("approval frame");
    (workspace, terminal, next)
}

/// INV-3/APV-4: hover paints a pointer target without changing the keyboard decision or focus.
#[test]
fn approval_hover_is_visual_only_and_repeated_motion_is_free() {
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal, _) = interaction_fixture(width, 30, "echo hello");
        let at = allow_button(&workspace, &terminal);
        let focus = workspace.state.focused(workspace.surfaces());
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Moved, at))
                .approval
                .is_none()
        );
        workspace.settled_draw(&mut terminal).expect("hover");
        assert!(
            workspace
                .state
                .approval_hovered(crate::ApprovalChoice::AllowOnce)
        );
        assert_eq!(
            workspace.state.approval().expect("card").selected,
            crate::ApprovalChoice::Deny
        );
        assert_eq!(workspace.state.focused(workspace.surfaces()), focus);
        assert_eq!(
            terminal.backend().buffer()[(at.x + 5, at.y)].fg,
            Palette::pastel()
                .style(crate::Role::Accent)
                .fg
                .expect("accent")
        );
        let frames = workspace.frames();
        workspace.handle(&mouse(MouseEventKind::Moved, at));
        workspace.settled_draw(&mut terminal).expect("unchanged");
        assert_eq!(workspace.frames(), frames);
        workspace.handle(&mouse(MouseEventKind::Moved, Point { x: 0, y: 0 }));
        assert!(
            !workspace
                .state
                .approval_hovered(crate::ApprovalChoice::AllowOnce)
        );
    }
}

/// INV-2/APV-4/PER-5: numbers act on displayed choices only while focused and preserve two-step grants.
#[test]
fn approval_numbers_follow_focus_and_the_current_choice_stage() {
    for (number, expected) in [
        ('1', ApprovalDecision::AllowOnce),
        ('3', ApprovalDecision::Deny),
    ] {
        let (mut workspace, mut terminal, _) = interaction_fixture(88, 30, "echo hello");
        let decision = workspace
            .handle(&key(KeyCode::Char(number)))
            .approval
            .expect("number decides");
        assert_eq!(decision.decision, expected);
        workspace.settled_draw(&mut terminal).expect("submitting");
        assert!(
            workspace
                .handle(&key(KeyCode::Char(number)))
                .approval
                .is_none()
        );
    }
    let (mut workspace, mut terminal, _) = interaction_fixture(88, 30, "echo hello");
    assert!(
        workspace
            .handle(&key(KeyCode::Char('2')))
            .approval
            .is_none()
    );
    assert_eq!(
        workspace.state.approval().expect("scope").stage,
        crate::ApprovalStage::Remember
    );
    workspace.settled_draw(&mut terminal).expect("scope frame");
    assert!(matches!(
        workspace
            .handle(&key(KeyCode::Char('2')))
            .approval
            .expect("scope confirms")
            .decision,
        ApprovalDecision::AllowAndRemember {
            scope: plexmaton_core::PermissionScope::Project,
            ..
        }
    ));
    let (mut workspace, mut terminal, _) = interaction_fixture(88, 30, "echo hello");
    workspace.handle(&key(KeyCode::Esc));
    workspace.settled_draw(&mut terminal).expect("composer");
    assert!(
        workspace
            .handle(&key(KeyCode::Char('1')))
            .approval
            .is_none()
    );
    assert_eq!(workspace.state.composer().text(), "1");
}

/// SURF-4/APV-4/ENT-4: command inspection copies exact source and closes without authorizing it.
#[test]
fn command_modal_copies_exact_source_and_returns_to_the_pending_approval() {
    let source = (0..40)
        .map(|i| format!("printf '%s' \"line {i} λ\"\n"))
        .collect::<String>();
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal, mut next) = interaction_fixture(width, 30, &source);
        let bounds = workspace
            .surfaces
            .get(SurfaceId::Approval)
            .expect("card")
            .bounds;
        let inset = crate::surface::ContentInsets::for_surface(SurfaceId::Approval, bounds.height);
        let summary = Point {
            x: bounds.x + 3,
            y: bounds.y + 1 + inset.vertical,
        };
        assert_eq!(
            terminal.backend().buffer()[(summary.x, summary.y)].fg,
            Palette::pastel()
                .style(crate::Role::Accent)
                .fg
                .expect("clickable command")
        );
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), summary));
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), summary))
                .approval
                .is_none()
        );
        workspace.settled_draw(&mut terminal).expect("modal");
        assert_eq!(
            workspace.state.focused(workspace.surfaces()),
            Some(SurfaceId::CommandInspection)
        );
        assert_eq!(
            workspace
                .handle(&key(KeyCode::Char('c')))
                .copied
                .expect("copy")
                .text,
            source
        );
        assert!(
            workspace
                .handle(&key(KeyCode::Char('1')))
                .approval
                .is_none()
        );
        workspace.handle(&key(KeyCode::Down));
        workspace.settled_draw(&mut terminal).expect("scroll");
        assert!(
            workspace
                .surfaces
                .viewport(SurfaceId::CommandInspection)
                .expect("viewport")
                .offset
                > 0
        );
        workspace.handle(&key(KeyCode::Esc));
        workspace.settled_draw(&mut terminal).expect("back");
        assert_eq!(
            workspace.state.approval().expect("pending").selected,
            crate::ApprovalChoice::Deny
        );
        assert_eq!(
            workspace.state.focused(workspace.surfaces()),
            Some(SurfaceId::Approval)
        );
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('o'),
            KeyModifiers::CONTROL,
        )));
        workspace
            .settled_draw(&mut terminal)
            .expect("keyboard opens");
        let bounds = workspace
            .surfaces
            .get(SurfaceId::CommandInspection)
            .expect("modal")
            .bounds;
        let [copy, close] = crate::layout::command_inspection_controls(bounds);
        let at = Point {
            x: copy.x + 1,
            y: copy.y,
        };
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
        assert_eq!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), at))
                .copied
                .expect("copy icon")
                .text,
            source
        );
        let at = Point {
            x: close.x + 1,
            y: close.y,
        };
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
        workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), at));
        assert!(!workspace.state.command_inspection_open());
        workspace.settled_draw(&mut terminal).expect("closed");
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('o'),
            KeyModifiers::CONTROL,
        )));
        workspace.settled_draw(&mut terminal).expect("open again");
        emit(
            &mut workspace,
            &mut next,
            ConversationEvent::AttentionResolved {
                agent_id: agent(),
                attention_id: attention(0),
            },
        );
        assert!(!workspace.state.command_inspection_open());
        assert!(workspace.handle(&key(KeyCode::Char('c'))).copied.is_none());
    }
}

/// PER-5/APV-4: a held Enter cannot confirm the scope opened by its first press.
#[test]
fn approval_scope_confirmation_requires_a_distinct_enter_press() {
    let (mut workspace, mut terminal, _) = interaction_fixture(88, 30, "echo hello");
    workspace.handle(&key(KeyCode::Up));
    assert!(workspace.handle(&key(KeyCode::Enter)).approval.is_none());
    workspace.settled_draw(&mut terminal).expect("scope");
    let repeat = Event::Key(KeyEvent::new_with_kind(
        KeyCode::Enter,
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ));
    assert!(workspace.handle(&repeat).approval.is_none());
    assert_eq!(
        workspace.state.approval().expect("still reviewing").stage,
        crate::ApprovalStage::Remember
    );
    assert!(workspace.handle(&key(KeyCode::Enter)).approval.is_some());
}

/// SURF-3/SURF-4/APD-2: resolution below the Drawer cannot redirect its keys or strand return focus.
#[test]
fn drawer_keeps_focus_when_the_inspected_approval_resolves() {
    for next_approval in [false, true] {
        let (mut workspace, mut terminal, mut next) = interaction_fixture(88, 30, "echo hello");
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('o'),
            KeyModifiers::CONTROL,
        )));
        workspace.settled_draw(&mut terminal).expect("inspection");
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        )));
        workspace.settled_draw(&mut terminal).expect("drawer");
        if next_approval {
            emit(&mut workspace, &mut next, request(1));
        }
        emit(
            &mut workspace,
            &mut next,
            ConversationEvent::AttentionResolved {
                agent_id: agent(),
                attention_id: attention(0),
            },
        );
        workspace
            .settled_draw(&mut terminal)
            .expect("resolved underneath");
        assert_eq!(
            workspace.state.focused(workspace.surfaces()),
            Some(SurfaceId::Drawer)
        );
        assert!(!workspace.state.command_inspection_open());
        workspace.handle(&key(KeyCode::Esc));
        workspace
            .settled_draw(&mut terminal)
            .expect("drawer closes");
        assert_eq!(
            workspace.state.focused(workspace.surfaces()),
            Some(if next_approval {
                SurfaceId::Approval
            } else {
                SurfaceId::Composer
            })
        );
    }
}

/// APD-2/INV-11: a coalesced request with a reused approval ID cannot inherit a held command press.
#[test]
fn command_summary_press_pins_the_complete_request_identity() {
    let (mut workspace, mut terminal, mut next) = interaction_fixture(88, 30, "echo old");
    let bounds = workspace
        .surfaces
        .get(SurfaceId::Approval)
        .expect("card")
        .bounds;
    let inset = crate::surface::ContentInsets::for_surface(SurfaceId::Approval, bounds.height);
    let at = Point {
        x: bounds.x + 3,
        y: bounds.y + 1 + inset.vertical,
    };
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
    emit(
        &mut workspace,
        &mut next,
        ConversationEvent::ToolCallChanged {
            agent_id: agent(),
            item_id: plexmaton_core::TranscriptItemId::new("tool-replacement").expect("item"),
            item_revision: 0,
            call_id: ToolCallId::new("replacement").expect("call"),
            label: "exec_command".into(),
            status: plexmaton_core::ToolCallStatus::Queued,
            presentation: plexmaton_core::ToolPresentation {
                invocation: Some(plexmaton_core::ToolDetail::Command(Box::new(
                    plexmaton_core::CommandInvocation {
                        source: "echo replacement".into(),
                        workspace_root: "/workspace".into(),
                        timeout_ms: 10_000,
                    },
                ))),
                outcome: None,
            },
        },
    );
    let mut replacement = request(0);
    if let ConversationEvent::AttentionRequested {
        request: AttentionRequest::Approval { call_id, .. },
        ..
    } = &mut replacement
    {
        *call_id = ToolCallId::new("replacement").expect("call");
    }
    emit(&mut workspace, &mut next, replacement);
    assert!(
        workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), at))
            .approval
            .is_none()
    );
    workspace.settled_draw(&mut terminal).expect("replacement");
    assert!(!workspace.state.command_inspection_open());
}

/// APD-3/INV-11: the modal's copy button shares drag, focus-loss and resize cancellation.
#[test]
fn command_modal_copy_press_cancels_on_drag_focus_loss_and_resize() {
    for cancel in 0..3 {
        let (mut workspace, mut terminal, _) = interaction_fixture(88, 30, "echo hello");
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('o'),
            KeyModifiers::CONTROL,
        )));
        workspace.settled_draw(&mut terminal).expect("inspection");
        let bounds = workspace
            .surfaces
            .get(SurfaceId::CommandInspection)
            .expect("modal")
            .bounds;
        let copy = crate::layout::command_inspection_controls(bounds)[0];
        let at = Point {
            x: copy.x + 1,
            y: copy.y,
        };
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
        match cancel {
            0 => {
                workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), at));
            }
            1 => {
                workspace.handle(&Event::FocusLost);
            }
            _ => {
                workspace.handle(&Event::Resize(60, 30));
            }
        }
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), at))
                .copied
                .is_none()
        );
    }
}

/// APD-3/INV-11/SURF-4: capture cannot activate a button behind a later blocking layer.
#[test]
fn approval_and_command_presses_do_not_activate_beneath_the_drawer() {
    for action in 0..4 {
        let (mut workspace, mut terminal, _) = interaction_fixture(88, 30, "echo hello");
        let at = match action {
            0 => allow_button(&workspace, &terminal),
            1 => {
                let bounds = workspace
                    .surfaces
                    .get(SurfaceId::Approval)
                    .expect("card")
                    .bounds;
                let inset =
                    crate::surface::ContentInsets::for_surface(SurfaceId::Approval, bounds.height);
                Point {
                    x: bounds.x + 3,
                    y: bounds.y + 1 + inset.vertical,
                }
            }
            _ => {
                workspace.handle(&Event::Key(KeyEvent::new(
                    KeyCode::Char('o'),
                    KeyModifiers::CONTROL,
                )));
                workspace.settled_draw(&mut terminal).expect("inspection");
                let bounds = workspace
                    .surfaces
                    .get(SurfaceId::CommandInspection)
                    .expect("modal")
                    .bounds;
                let button = crate::layout::command_inspection_controls(bounds)[action - 2];
                Point {
                    x: button.x + 1,
                    y: button.y,
                }
            }
        };
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
        workspace.handle(&Event::Key(KeyEvent::new(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        )));
        workspace
            .settled_draw(&mut terminal)
            .expect("drawer covers pressed target");
        let outcome = workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), at));
        assert!(outcome.approval.is_none());
        assert!(outcome.copied.is_none());
        assert_eq!(workspace.state.command_inspection_open(), action >= 2);
        assert_eq!(
            workspace.state.focused(workspace.surfaces()),
            Some(SurfaceId::Drawer)
        );
    }
}

/// INV-11/APV-4: leaving and returning by keyboard cannot rearm a held approval press.
#[test]
fn approval_press_is_cancelled_by_a_keyboard_focus_round_trip() {
    let (mut workspace, mut terminal, _) = interaction_fixture(88, 30, "echo hello");
    let at = allow_button(&workspace, &terminal);
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
    workspace.handle(&key(KeyCode::Tab));
    workspace
        .settled_draw(&mut terminal)
        .expect("leave approval");
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::SHIFT,
    )));
    workspace
        .settled_draw(&mut terminal)
        .expect("return to approval");
    assert_eq!(
        workspace.state.focused(workspace.surfaces()),
        Some(SurfaceId::Approval)
    );
    assert!(
        workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), at))
            .approval
            .is_none()
    );
}

/// PER-10/APV-4: a second shortcut cannot confirm a scope that has not reached the screen yet.
#[test]
fn numbered_scope_confirmation_waits_for_the_scope_frame() {
    let (mut workspace, mut terminal, _) = interaction_fixture(88, 30, "echo hello");
    assert!(
        workspace
            .handle(&key(KeyCode::Char('2')))
            .approval
            .is_none()
    );
    assert!(
        workspace
            .handle(&key(KeyCode::Char('2')))
            .approval
            .is_none()
    );
    workspace
        .settled_draw(&mut terminal)
        .expect("scope visible");
    assert!(
        workspace
            .handle(&key(KeyCode::Char('2')))
            .approval
            .is_some()
    );
}

/// APV-4/FR-3: a newly advanced request cannot inherit a decision before its frame is painted.
#[test]
fn numbered_decision_waits_for_a_replacement_approval_frame() {
    let (mut workspace, mut terminal, mut next) = interaction_fixture(88, 30, "echo hello");
    emit(&mut workspace, &mut next, request(1));
    emit(
        &mut workspace,
        &mut next,
        ConversationEvent::AttentionResolved {
            agent_id: agent(),
            attention_id: attention(0),
        },
    );
    assert!(
        workspace
            .handle(&key(KeyCode::Char('1')))
            .approval
            .is_none()
    );
    workspace
        .settled_draw(&mut terminal)
        .expect("replacement visible");
    assert_eq!(
        workspace
            .handle(&key(KeyCode::Char('1')))
            .approval
            .expect("new explicit decision")
            .approval_id
            .as_str(),
        "approval-1"
    );
}
