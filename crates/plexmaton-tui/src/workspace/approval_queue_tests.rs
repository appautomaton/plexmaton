use super::*;
use crate::{ApprovalIntent, Point, SurfaceId};
use plexmaton_core::{
    AgentStatus, ApprovalDecision, ApprovalId, AttentionId, AttentionRequest, ConversationEvent,
    EventSequence, ToolCallId, ToolCapability,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
};

pub(super) fn agent() -> AgentId {
    AgentId::new("primary").expect("agent")
}
pub(super) fn attention(index: usize) -> AttentionId {
    AttentionId::new(format!("ask-{index}")).expect("attention")
}
pub(super) fn request(index: usize) -> ConversationEvent {
    ConversationEvent::AttentionRequested {
        agent_id: agent(),
        attention_id: attention(index),
        request: AttentionRequest::Approval {
            reason: plexmaton_core::ApprovalReason::PermissionRequired,
            remember: None,
            approval_id: ApprovalId::new(format!("approval-{index}")).expect("approval"),
            call_id: ToolCallId::new(format!("call-{index}")).expect("call"),
            tool: "exec_command".into(),
            capabilities: vec![ToolCapability::ProcessSpawn],
            detail: format!("Command {index}: inspect the workspace"),
        },
    }
}
pub(super) fn emit(workspace: &mut Workspace, next: &mut u64, event: ConversationEvent) {
    workspace.emit(vec![ConversationEventEnvelope {
        sequence: EventSequence::new(*next),
        event,
    }]);
    *next += 1;
}

/// ATT-1/ATT-3: later parallel requests cannot replace the current decision or strand earlier ones in the band.
#[test]
fn parallel_primary_approvals_stay_inline_and_advance_in_arrival_order() {
    for width in [60, 95, 120] {
        let mut workspace = Workspace::with_palette(Palette::pastel());
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
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
        workspace.return_input(agent(), "keep this draft".into());
        emit(&mut workspace, &mut next, request(0));
        workspace.settled_draw(&mut terminal).expect("first card");
        workspace
            .state
            .decide_approval(ApprovalIntent::Move(Direction::Backward));
        for index in [1, 2] {
            emit(&mut workspace, &mut next, request(index));
        }
        assert_eq!(
            workspace.state().approval().expect("current").attention_id,
            &attention(0)
        );
        assert_eq!(
            workspace
                .state()
                .approval()
                .expect("choice preserved")
                .selected,
            crate::ApprovalChoice::AllowOnce
        );
        for index in 0..3 {
            workspace.settled_draw(&mut terminal).expect("inline card");
            assert!(workspace.surfaces().get(SurfaceId::Attention).is_none());
            assert_eq!(workspace.state().attention_listed_count(), 0);
            let approval = workspace.state().approval().expect("pending inline");
            assert_eq!(approval.attention_id, &attention(index));
            if index > 0 {
                assert_eq!(approval.selected, crate::ApprovalChoice::Deny);
            }
            let decision = workspace
                .state
                .decide_approval(ApprovalIntent::Decide)
                .expect("decision");
            assert_eq!(decision.approval_id.as_str(), format!("approval-{index}"));
            assert_eq!(
                workspace
                    .state()
                    .approval()
                    .expect("wait for producer")
                    .attention_id,
                &attention(index)
            );
            emit(
                &mut workspace,
                &mut next,
                ConversationEvent::AttentionResolved {
                    agent_id: agent(),
                    attention_id: attention(index),
                },
            );
        }
        workspace
            .settled_draw(&mut terminal)
            .expect("finished batch");
        assert!(workspace.state().approval().is_none());
        assert_eq!(workspace.state().attention_count(), 0);
        assert_eq!(workspace.state().composer().text(), "keep this draft");
        assert_eq!(
            workspace.state().focused(workspace.surfaces()),
            Some(SurfaceId::Composer)
        );
    }
}

pub(super) fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
pub(super) fn mouse(kind: MouseEventKind, at: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: at.x,
        row: at.y,
        modifiers: KeyModifiers::NONE,
    })
}
pub(super) fn allow_button(workspace: &Workspace, terminal: &Terminal<TestBackend>) -> Point {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::Approval)
        .expect("card")
        .bounds;
    let row = (bounds.y..bounds.bottom())
        .find(|y| {
            (bounds.x..bounds.right())
                .map(|x| terminal.backend().buffer()[(x, *y)].symbol())
                .collect::<String>()
                .contains("Allow once")
        })
        .expect("visible button");
    Point {
        x: bounds.x + 3,
        y: row,
    }
}

/// ATT-1/ATT-3/INV-6: Escape keeps primary approval in its own conversation; clicking/Tab can answer there.
#[test]
fn primary_approval_escape_returns_to_composer_without_creating_attention_ui() {
    for width in [60, 95, 120] {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
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
        emit(&mut workspace, &mut next, request(0));
        emit(&mut workspace, &mut next, request(1));
        workspace
            .settled_draw(&mut terminal)
            .expect("inline approvals");
        workspace.handle(&key(KeyCode::Esc));
        workspace
            .settled_draw(&mut terminal)
            .expect("composer focus");
        assert_eq!(
            workspace.state().focused(workspace.surfaces()),
            Some(SurfaceId::Composer)
        );
        assert!(workspace.state().approval().is_some());
        assert_eq!(workspace.state().attention_listed_count(), 0);
        assert!(workspace.surfaces().get(SurfaceId::Attention).is_none());
        workspace.handle(&Event::Paste("draft while approval waits".into()));
        workspace.settled_draw(&mut terminal).expect("draft");
        workspace.handle(&key(KeyCode::Tab));
        workspace.settled_draw(&mut terminal).expect("back to card");
        assert_eq!(
            workspace.state().focused(workspace.surfaces()),
            Some(SurfaceId::Approval)
        );
        workspace.handle(&key(KeyCode::Up));
        let decision = workspace
            .handle(&key(KeyCode::Enter))
            .approval
            .expect("keyboard decision");
        assert_eq!(decision.approval_id.as_str(), "approval-0");
        assert_eq!(decision.decision, ApprovalDecision::AllowOnce);
        emit(
            &mut workspace,
            &mut next,
            ConversationEvent::AttentionResolved {
                agent_id: agent(),
                attention_id: attention(0),
            },
        );
        workspace.settled_draw(&mut terminal).expect("next card");
        workspace.handle(&key(KeyCode::Esc));
        workspace
            .settled_draw(&mut terminal)
            .expect("composer again");
        let at = allow_button(&workspace, &terminal);
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
        let decision = workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), at))
            .approval
            .expect("pointer decision");
        assert_eq!(decision.approval_id.as_str(), "approval-1");
        assert_eq!(decision.decision, ApprovalDecision::AllowOnce);
        assert_eq!(
            workspace.state().composer().text(),
            "draft while approval waits"
        );
        assert!(workspace.surfaces().get(SurfaceId::Attention).is_none());
    }
}

/// APV-4/ATT-3: a cancelled or stale pointer press cannot approve the next request occupying the same cells.
#[test]
fn approval_pointer_refuses_drag_focus_loss_resize_and_replaced_request() {
    let mut workspace = Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(95, 24)).expect("terminal");
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
    emit(&mut workspace, &mut next, request(0));
    emit(&mut workspace, &mut next, request(1));
    workspace.settled_draw(&mut terminal).expect("card");
    let at = allow_button(&workspace, &terminal);
    for cancellation in [
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            Point { x: at.x + 1, ..at },
        ),
        Event::FocusLost,
        Event::Resize(95, 24),
    ] {
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
        workspace.handle(&cancellation);
        assert!(
            workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), at))
                .approval
                .is_none()
        );
        workspace.handle(&Event::FocusGained);
        workspace.settled_draw(&mut terminal).expect("cancelled");
    }
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
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
        .expect("new request at same position");
    assert!(
        workspace
            .handle(&mouse(MouseEventKind::Up(MouseButton::Left), at))
            .approval
            .is_none()
    );
    assert_eq!(
        workspace
            .state()
            .approval()
            .expect("still pending")
            .attention_id,
        &attention(1)
    );
}

/// ATT-1/ATT-3: keyboard activation uses the filtered visible queue, even when its first raw item is primary.
#[test]
fn attention_keyboard_activates_the_visible_worker_and_escape_restores_primary_card() {
    let mut workspace = Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
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
    let worker = AgentId::new("worker").expect("worker");
    emit(
        &mut workspace,
        &mut next,
        ConversationEvent::AgentCreated {
            agent_id: worker.clone(),
            label: "Worker".into(),
            status: AgentStatus::Waiting,
        },
    );
    emit(&mut workspace, &mut next, request(0));
    let mut background = request(1);
    if let ConversationEvent::AttentionRequested { agent_id, .. } = &mut background {
        *agent_id = worker.clone();
    }
    emit(&mut workspace, &mut next, background);
    workspace.draw(&mut terminal).expect("frame");
    assert_eq!(
        workspace
            .state()
            .attention_listed()
            .map(|item| &item.agent_id)
            .collect::<Vec<_>>(),
        [&worker]
    );
    workspace
        .state
        .attend(&workspace.surfaces, crate::AttentionIntent::GoTo);
    assert_eq!(
        workspace
            .state()
            .approval()
            .expect("visible worker")
            .agent_id,
        &worker
    );
    workspace.draw(&mut terminal).expect("worker card");
    workspace.state.dismiss(&workspace.surfaces);
    assert_eq!(
        workspace
            .state()
            .approval()
            .expect("primary returns")
            .attention_id,
        &attention(0)
    );
}

/// PER-10/PER-5: a growing draft cannot leave an actionable but unreadable remembered scope.
#[test]
fn per_10_keyboard_and_pointer_cannot_confirm_a_scope_clipped_by_the_draft() {
    for pointer in [false, true] {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("terminal");
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
        let draft = "first line\nsecond line\nthird line";
        workspace.return_input(agent(), draft.into());
        let mut event = request(0);
        let ConversationEvent::AttentionRequested {
            request: AttentionRequest::Approval { remember, .. },
            ..
        } = &mut event
        else {
            panic!("approval");
        };
        *remember = Some(plexmaton_core::RememberPermissionOffer {
            id: plexmaton_core::PermissionOfferId::new(1),
            label: "git fetch …; same cwd/environment".into(),
            note: None,
            scopes: plexmaton_core::PermissionScopes::SessionAndProject,
        });
        emit(&mut workspace, &mut next, event);
        workspace.draw(&mut terminal).expect("review");
        workspace.handle(&key(KeyCode::Up));
        assert!(workspace.handle(&key(KeyCode::Enter)).approval.is_none());
        workspace.draw(&mut terminal).expect("scope");
        assert_eq!(
            workspace.state.approval().expect("view").stage,
            crate::ApprovalStage::Remember
        );
        let bounds = workspace
            .surfaces
            .get(SurfaceId::Approval)
            .expect("card")
            .bounds;
        let text = crate::test_support::snapshot_text(terminal.backend().buffer(), bounds);
        assert!(text.contains("(resize)"), "{text}");
        let outcome = if pointer {
            let row = text
                .lines()
                .position(|row| row.contains("This Session"))
                .expect("disabled row");
            let at = Point {
                x: bounds.x + 4,
                y: bounds.y + u16::try_from(row).expect("row"),
            };
            workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
            workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), at))
        } else {
            workspace.handle(&key(KeyCode::Enter))
        };
        assert!(outcome.approval.is_none());
        assert!(
            workspace
                .handle(&key(KeyCode::Char('1')))
                .approval
                .is_none()
        );
        assert!(
            workspace
                .handle(&key(KeyCode::Char('2')))
                .approval
                .is_none()
        );
        assert_eq!(
            workspace.state.approval().expect("still pending").stage,
            crate::ApprovalStage::Remember
        );
        assert_eq!(workspace.state.composer().text(), draft);
        workspace.handle(&Event::Resize(120, 30));
        let mut large = Terminal::new(TestBackend::new(120, 30)).expect("resized terminal");
        workspace.draw(&mut large).expect("readable scope");
        let outcome = workspace.handle(&key(KeyCode::Enter));
        assert!(matches!(
            outcome.approval.expect("reviewed scope").decision,
            ApprovalDecision::AllowAndRemember {
                scope: plexmaton_core::PermissionScope::Session,
                ..
            }
        ));
        assert_eq!(workspace.state.composer().text(), draft);
    }
}
