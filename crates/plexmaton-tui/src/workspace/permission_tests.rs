//! Permission controls in their two places (PER-7): the Session's behind `/permissions` in the
//! composer menu, the Project's on the Drawer's page. One owner, one revision, one reviewed
//! intent from either place.
use super::*;
use crate::{
    PermissionRequest, SurfaceId,
    test_support::{canonical_runtime, region_text, snapshot_text},
};
use plexmaton_core::{
    CodingSessionId, NativeFilePreset, PermissionAction, PermissionChangeError, PermissionGrantId,
    PermissionGrantView, PermissionRevision, PermissionScope, PermissionStateView,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::Rect,
};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn view() -> PermissionStateView {
    PermissionStateView {
        configuration: None,
        trusted_config: None,
        project: plexmaton_core::ProjectPermissionSource::Disabled,
        revision: PermissionRevision::initial(CodingSessionId::new("coding").expect("id")),
        native_files: NativeFilePreset::Disabled,
        grants: Vec::new(),
    }
}
fn grant(name: &str, scope: PermissionScope, label: &str) -> PermissionGrantView {
    PermissionGrantView {
        id: PermissionGrantId::new(name).expect("grant"),
        scope,
        label: label.to_owned(),
    }
}
/// One Session preset, one other Session grant, one Project grant.
fn split_view() -> PermissionStateView {
    let mut view = view();
    view.project = plexmaton_core::ProjectPermissionSource::Available;
    view.native_files = NativeFilePreset::Enabled(PermissionGrantId::new("preset").expect("id"));
    view.grants = vec![
        grant("preset", PermissionScope::Session, "Native create/edit"),
        grant("ls", PermissionScope::Session, "Command prefix: ls"),
        grant(
            "fetch",
            PermissionScope::Project,
            "Exact command: git fetch",
        ),
    ];
    view
}

/// One primary conversation and nothing else on screen: no rail, no strip.
fn primary_only() -> Vec<ConversationEventEnvelope> {
    use plexmaton_core::{AgentStatus, ConversationEvent, EventSequence};
    vec![ConversationEventEnvelope {
        sequence: EventSequence::new(1),
        event: ConversationEvent::AgentCreated {
            agent_id: AgentId::new("primary").expect("agent"),
            label: "Plexmaton".into(),
            status: AgentStatus::Idle,
        },
    }]
}

/// A drawn workspace with the caret in the composer and the Session's rows listed behind
/// `/permissions`, once the owner's view has landed.
fn menu_fixture(
    width: u16,
    height: u16,
    view: PermissionStateView,
) -> (Workspace, Terminal<TestBackend>) {
    menu_fixture_over(canonical_runtime().ready(u64::MAX), width, height, view)
}

fn menu_fixture_over(
    events: Vec<ConversationEventEnvelope>,
    width: u16,
    height: u16,
    view: PermissionStateView,
) -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    workspace.emit(events);
    workspace.settled_draw(&mut terminal).expect("draw");
    for _ in 0..=workspace.surfaces.len() {
        if workspace.state.focused(&workspace.surfaces) == Some(SurfaceId::Composer) {
            break;
        }
        workspace.handle(&key(KeyCode::Tab));
        workspace.settled_draw(&mut terminal).expect("draw");
    }
    for character in "/permissions".chars() {
        workspace.handle(&key(KeyCode::Char(character)));
    }
    assert_eq!(
        workspace.handle(&key(KeyCode::Enter)).permission,
        Some(PermissionRequest::Refresh),
        "the rows are asked for once"
    );
    workspace.update_permissions(Ok(view), None);
    workspace.settled_draw(&mut terminal).expect("listed");
    (workspace, terminal)
}
/// The menu and the composer beneath it, as one cropped frame.
fn menu(workspace: &Workspace, terminal: &Terminal<TestBackend>) -> String {
    let menu = workspace
        .surfaces()
        .get(SurfaceId::ComposerMenu)
        .expect("the menu is registered")
        .bounds;
    let composer = workspace
        .surfaces()
        .get(SurfaceId::Composer)
        .expect("composer")
        .bounds;
    region_text(
        terminal.backend().buffer(),
        Rect::new(
            menu.x,
            menu.y,
            menu.width,
            composer.bottom().saturating_sub(menu.y),
        ),
    )
}
fn press_release(workspace: &mut Workspace, bounds: Rect, drawn: &str, text: &str) -> Outcome {
    let (row, line) = drawn
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(text))
        .unwrap_or_else(|| panic!("{text:?} is a visible row:\n{drawn}"));
    let column = line.chars().position(|c| c == '>').unwrap_or(3);
    let event = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: bounds.x + u16::try_from(column).expect("column"),
            row: bounds.y + u16::try_from(row).expect("row"),
            modifiers: KeyModifiers::NONE,
        })
    };
    workspace.handle(&event(MouseEventKind::Down(MouseButton::Left)));
    workspace.handle(&event(MouseEventKind::Up(MouseButton::Left)))
}
fn click_menu(workspace: &mut Workspace, terminal: &Terminal<TestBackend>, text: &str) -> Outcome {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::ComposerMenu)
        .expect("menu")
        .bounds;
    let drawn = region_text(terminal.backend().buffer(), bounds);
    press_release(workspace, bounds, &drawn, text)
}
fn panel(workspace: &Workspace, terminal: &Terminal<TestBackend>) -> String {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::Drawer)
        .expect("permissions")
        .bounds;
    snapshot_text(terminal.backend().buffer(), bounds)
}
fn click(workspace: &mut Workspace, terminal: &Terminal<TestBackend>, text: &str) -> Outcome {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::Drawer)
        .expect("permissions")
        .bounds;
    let drawn = panel(workspace, terminal);
    press_release(workspace, bounds, &drawn, text)
}
fn change(outcome: Outcome) -> plexmaton_core::PermissionIntent {
    match outcome.permission {
        Some(PermissionRequest::Change(intent)) => intent,
        other => panic!("a confirmed row leaves as a change, not {other:?}"),
    }
}

/// PER-7/SURF-3: keyboard and drawn-row clicks in the menu emit one reviewed revision; the
/// producer owns completion, `Escape` returns one layer, and closing withdraws the place.
#[test]
fn per_7_permission_controls_review_cancel_submit_and_refresh_by_identity() {
    for width in [120, 95, 60, 48] {
        for pointer in [false, true] {
            let (mut workspace, mut terminal) = menu_fixture(width, 24, view());
            let review = if pointer {
                click_menu(&mut workspace, &terminal, "Enable native")
            } else {
                workspace.handle(&key(KeyCode::Enter))
            };
            assert!(review.permission.is_none(), "opening review grants nothing");
            workspace.settled_draw(&mut terminal).expect("confirmation");
            assert!(menu(&workspace, &terminal).contains("> Back"));
            workspace.handle(&key(KeyCode::Esc));
            workspace.settled_draw(&mut terminal).expect("back");
            assert!(menu(&workspace, &terminal).contains("Enable native"));
            workspace.handle(&key(KeyCode::Enter));
            workspace.handle(&key(KeyCode::Up));
            workspace
                .settled_draw(&mut terminal)
                .expect("chosen confirmation");
            let confirmed = if pointer {
                click_menu(&mut workspace, &terminal, "> Enable for")
            } else {
                workspace.handle(&key(KeyCode::Enter))
            };
            let intent = change(confirmed);
            assert_eq!(intent.expected, view().revision);
            assert_eq!(intent.action, PermissionAction::EnableNativeFiles);
            assert!(workspace.handle(&key(KeyCode::Enter)).permission.is_none());
            workspace.settled_draw(&mut terminal).expect("submitting");
            assert!(menu(&workspace, &terminal).contains("Applying change"));
            let mut fresh = view();
            fresh.revision = fresh.revision.next().expect("new revision");
            workspace.update_permissions(
                Ok(fresh.clone()),
                Some(Err(PermissionChangeError::StaleRevision)),
            );
            workspace.settled_draw(&mut terminal).expect("refused");
            assert!(menu(&workspace, &terminal).contains("permissions changed"));
            workspace.handle(&key(KeyCode::Enter));
            workspace.handle(&key(KeyCode::Up));
            assert_eq!(
                change(workspace.handle(&key(KeyCode::Enter))).expected,
                fresh.revision
            );
            workspace.handle(&key(KeyCode::Esc));
            assert!(
                !workspace.permissions_open(),
                "closing the menu withdraws the place"
            );
            workspace.update_permissions(Ok(view()), Some(Ok(())));
            assert!(
                !workspace.permissions_open(),
                "late completion cannot reopen a dismissed place"
            );
            assert_eq!(
                workspace.state.composer().text(),
                "/permissions ",
                "Escape keeps the draft"
            );
        }
    }
}

/// PER-7/SKP-4: reviewable three-width frames and a smallest-terminal confirmation keep the
/// Session's actions visible in the menu.
#[test]
fn per_7_permission_controls_frames_keep_scope_and_confirmation_visible() {
    for (width, name) in [(120, "wide"), (95, "medium"), (60, "narrow")] {
        let (mut workspace, mut terminal) = menu_fixture(width, 30, view());
        let mut drawn = menu(&workspace, &terminal);
        assert!(
            drawn.contains("Enter review") && drawn.contains("Esc close"),
            "{drawn}"
        );
        workspace.handle(&key(KeyCode::Enter));
        workspace.settled_draw(&mut terminal).expect("review");
        let confirmation = menu(&workspace, &terminal);
        for text in [
            "create/edit",
            "configuration",
            "metadata",
            "Commands",
            "Enable for this Session",
            "> Back",
        ] {
            assert!(
                confirmation.contains(text),
                "{name}: {text}: {confirmation}"
            );
        }
        drawn.push_str("\n\n");
        drawn.push_str(&confirmation);
        crate::test_support::assert_frame(&format!("permission-controls-{name}"), &drawn);
    }
    // The smallest terminal, with one conversation: the heading gives way to the rows and ends
    // in `…`, and the question, both rows and the keys stay visible.
    let (mut workspace, mut terminal) = menu_fixture_over(primary_only(), 48, 12, view());
    workspace.handle(&key(KeyCode::Enter));
    workspace
        .settled_draw(&mut terminal)
        .expect("small confirm");
    let shown = menu(&workspace, &terminal);
    for text in [
        "create/edit",
        "…",
        "Enable for this Session",
        "> Back",
        "Esc back",
    ] {
        assert!(shown.contains(text), "{shown}");
    }
}

/// PER-7/PER-8: a Session grant is offered only in the menu and a Project grant only in the
/// Drawer, and revoking from either place leaves the same reviewed intent for the one owner.
#[test]
fn session_rows_live_in_the_menu_and_project_rows_in_the_drawer() {
    let (mut workspace, mut terminal) = menu_fixture(95, 30, split_view());
    let listed = menu(&workspace, &terminal);
    assert!(
        listed.contains("Turn off Session file changes…")
            && listed.contains("Revoke Session: Command prefix: ls"),
        "{listed}"
    );
    assert!(!listed.contains("Project"), "{listed}");
    workspace.handle(&key(KeyCode::Down));
    workspace.handle(&key(KeyCode::Enter));
    workspace.settled_draw(&mut terminal).expect("review");
    assert!(menu(&workspace, &terminal).contains("Command prefix: ls"));
    workspace.handle(&key(KeyCode::Up));
    let revoked = change(workspace.handle(&key(KeyCode::Enter)));
    assert_eq!(revoked.expected, split_view().revision);
    assert_eq!(
        revoked.action,
        PermissionAction::Revoke(PermissionGrantId::new("ls").expect("id"))
    );
    workspace.handle(&key(KeyCode::Esc));

    workspace.open_permissions();
    workspace.update_permissions(Ok(split_view()), None);
    workspace.settled_draw(&mut terminal).expect("page");
    let page = panel(&workspace, &terminal);
    assert!(
        page.contains("Revoke Project: Exact command: git fetch")
            && page.contains("Refresh permissions"),
        "{page}"
    );
    assert!(
        !page.contains("Revoke Session") && !page.contains("Session file changes"),
        "{page}"
    );
    assert!(
        click(&mut workspace, &terminal, "Revoke Project")
            .permission
            .is_none()
    );
    workspace.settled_draw(&mut terminal).expect("review");
    workspace.handle(&key(KeyCode::Up));
    let revoked = change(workspace.handle(&key(KeyCode::Enter)));
    assert_eq!(revoked.expected, split_view().revision);
    assert_eq!(
        revoked.action,
        PermissionAction::Revoke(PermissionGrantId::new("fetch").expect("id"))
    );
}

fn project_view() -> PermissionStateView {
    let mut view = view();
    view.project = plexmaton_core::ProjectPermissionSource::Available;
    view.configuration = Some(plexmaton_core::ProjectConfigurationView {
        fingerprint: [0xab; 32],
        trusted: false,
        rules: vec![
            plexmaton_core::PermissionRuleView {
                action: plexmaton_core::PermissionRuleAction::Allow,
                label: "Exact command: \"git fetch origin\"; fixed cwd and environment".to_owned(),
            },
            plexmaton_core::PermissionRuleView {
                action: plexmaton_core::PermissionRuleAction::Ask,
                label: "Native create/edit; excludes agent controls and Git metadata".to_owned(),
            },
        ],
    });
    view
}

/// PER-8/INV-3: every configured scope is reachable; scrolling cannot apply trust or move the footer.
#[test]
fn per_8_project_rule_review_scrolls_full_scopes_before_separate_confirmation() {
    for width in [120, 95, 60, 48] {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
        let mut view = project_view();
        view.configuration.as_mut().expect("configuration").rules[0].label = format!(
            "Exact command: head {} final-rule-tail",
            "long literal argument ".repeat(90)
        );
        workspace.open_permissions();
        workspace.update_permissions(Ok(view.clone()), None);
        workspace.draw(&mut terminal).expect("browse");
        assert!(
            click(&mut workspace, &terminal, "Review project")
                .permission
                .is_none()
        );
        workspace.draw(&mut terminal).expect("review");
        let bounds = workspace
            .surfaces()
            .get(SurfaceId::Drawer)
            .expect("review bounds")
            .bounds;
        let footer = crate::render::permission_review::choice_row(bounds);
        let initial = panel(&workspace, &terminal);
        assert!(
            initial.contains(".plexmaton/config.toml")
                && initial.contains("Continue to activation"),
            "{initial}"
        );
        let mut seen_tail = false;
        let mut seen_last_rule = false;
        for _ in 0..200 {
            let before = workspace
                .surfaces()
                .viewport(SurfaceId::Drawer)
                .expect("viewport")
                .offset;
            let outcome = workspace.handle(&key(KeyCode::Down));
            assert!(outcome.permission.is_none());
            workspace.draw(&mut terminal).expect("scroll");
            let text = panel(&workspace, &terminal);
            seen_tail |= text.contains("final-rule-tail");
            seen_last_rule |= text.contains("2. Ask:");
            assert_eq!(
                crate::render::permission_review::choice_row(
                    workspace
                        .surfaces()
                        .get(SurfaceId::Drawer)
                        .expect("surface")
                        .bounds
                ),
                footer
            );
            let after = workspace
                .surfaces()
                .viewport(SurfaceId::Drawer)
                .expect("viewport")
                .offset;
            if before == after {
                break;
            }
        }
        assert!(
            seen_tail && seen_last_rule,
            "every complete rule must be readable at {width}"
        );
        assert!(
            click(&mut workspace, &terminal, "Continue to activation")
                .permission
                .is_none()
        );
        workspace.draw(&mut terminal).expect("confirmation");
        assert!(panel(&workspace, &terminal).contains("> Back"));
        workspace.handle(&key(KeyCode::Up));
        let intent = change(workspace.handle(&key(KeyCode::Enter)));
        assert_eq!(intent.expected, view.revision);
        assert_eq!(
            intent.action,
            PermissionAction::TrustProjectConfiguration([0xab; 32])
        );
        assert!(
            workspace.handle(&key(KeyCode::Enter)).permission.is_none(),
            "submission is owned"
        );
    }
}

/// PER-8/DRW-2: real three-width frames keep project lifetime, source, rules and explicit activation legible.
#[test]
fn per_8_project_trust_frames_show_source_scopes_and_confirmation() {
    for (width, name) in [(120, "wide"), (95, "medium"), (60, "narrow")] {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
        workspace.open_permissions();
        workspace.update_permissions(Ok(project_view()), None);
        workspace.draw(&mut terminal).expect("browse");
        let mut text = panel(&workspace, &terminal);
        click(&mut workspace, &terminal, "Review project");
        workspace.draw(&mut terminal).expect("rules");
        text.push_str("\n\n");
        text.push_str(&panel(&workspace, &terminal));
        workspace.handle(&key(KeyCode::Enter));
        workspace.draw(&mut terminal).expect("confirmation");
        text.push_str("\n\n");
        text.push_str(&panel(&workspace, &terminal));
        for required in [
            "Project grants",
            ".plexmaton/config.toml",
            "git fetch origin",
            "Activate these Allow rules",
            "> Back",
        ] {
            assert!(text.contains(required), "{name}: {required}: {text}");
        }
        crate::test_support::assert_frame(&format!("project-trust-{name}"), &text);
    }
}

/// PER-6/TR-1/TR-3: the partial receipt stays beside its call, with no event revision or copy mutation.
#[test]
fn per_6_saved_project_receipt_frames_are_local_to_the_call_and_never_copied() {
    use plexmaton_core::{
        AgentStatus, ConversationEvent, EventSequence, PermissionGrantId, SavedProjectPermission,
        ToolCallId, ToolCallStatus, ToolPresentation, TranscriptItemId,
    };
    for (width, name) in [(120, "wide"), (95, "medium"), (60, "narrow")] {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
        let agent = AgentId::new("primary").expect("agent");
        let call_id = ToolCallId::new("call").expect("call");
        let item_id = TranscriptItemId::new("tool").expect("item");
        workspace.emit(vec![ConversationEventEnvelope {
            sequence: EventSequence::new(1),
            event: ConversationEvent::AgentCreated {
                agent_id: agent.clone(),
                label: "Plexmaton".into(),
                status: AgentStatus::Idle,
            },
        }]);
        for (revision, status) in [(0, ToolCallStatus::Queued), (1, ToolCallStatus::Cancelled)] {
            workspace.emit(vec![ConversationEventEnvelope {
                sequence: EventSequence::new(revision + 2),
                event: ConversationEvent::ToolCallChanged {
                    agent_id: agent.clone(),
                    item_id: item_id.clone(),
                    item_revision: revision,
                    call_id: call_id.clone(),
                    label: "exec_command".into(),
                    status,
                    presentation: ToolPresentation {
                        invocation: Some(plexmaton_core::ToolDetail::Text {
                            source: "printf hit > effect".into(),
                            omitted_bytes: 0,
                        }),
                        outcome: None,
                    },
                },
            }]);
        }
        workspace
            .state
            .begin_selection(SurfaceId::Transcript, agent.clone(), 0);
        let copy = workspace.copy_selection();
        assert!(copy.is_some());
        workspace
            .settled_draw(&mut terminal)
            .expect("measure before receipt");
        let receipt = SavedProjectPermission {
            call_id,
            grant: PermissionGrantId::new("saved-grant").expect("grant"),
        };
        workspace.report_saved_project_permission(&agent, receipt.clone());
        let once = workspace.state.clone();
        workspace.report_saved_project_permission(&agent, receipt);
        assert_eq!(workspace.state, once, "repeat receipt costs no revision");
        assert_eq!(workspace.copy_selection(), copy);
        assert_eq!(workspace.state.notices().count(), 0);
        assert_eq!(
            workspace
                .state
                .primary_agent()
                .expect("agent")
                .tools()
                .next()
                .expect("tool")
                .revision,
            1
        );
        workspace.settled_draw(&mut terminal).expect("receipt");
        let bounds = workspace
            .surfaces()
            .get(SurfaceId::Transcript)
            .expect("transcript")
            .bounds;
        let text = snapshot_text(terminal.backend().buffer(), bounds);
        for required in [
            "exec_command",
            "Project permission saved",
            "tool did not run",
            "Review it under Ctrl-P · Permissions",
        ] {
            assert!(text.contains(required), "{name}: {required}: {text}");
        }
        crate::test_support::assert_frame(&format!("project-permission-receipt-{name}"), &text);
    }
}

/// INV-3/PER-7: hover selects a revisioned permission without applying it, in both places.
#[test]
fn permission_hover_selects_without_granting_and_confirmation_starts_on_back() {
    use crate::{
        Point,
        state::{MenuRow, permissions::PermissionChoice},
    };
    for drawer in [false, true] {
        let (mut workspace, mut terminal) = menu_fixture(88, 30, split_view());
        let (surface, grant) = if drawer {
            workspace.handle(&key(KeyCode::Esc));
            workspace.open_permissions();
            workspace.update_permissions(Ok(split_view()), None);
            (SurfaceId::Drawer, "fetch")
        } else {
            (SurfaceId::ComposerMenu, "ls")
        };
        workspace
            .settled_draw(&mut terminal)
            .expect("permission list");
        let bounds = workspace.surfaces.get(surface).expect("surface").bounds;
        let choice = PermissionChoice::Review(PermissionAction::Revoke(
            PermissionGrantId::new(grant).expect("grant"),
        ));
        let at = (bounds.y..bounds.bottom())
            .map(|y| Point { x: bounds.x + 5, y })
            .find(|at| {
                if drawer {
                    workspace.drawer_hit(*at)
                        == Some(drawer::DrawerChoice::Permission(choice.clone()))
                } else {
                    workspace.menu_hit(*at) == Some(MenuRow::Permission(choice.clone()))
                }
            })
            .expect("grant row");
        let event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: at.x,
            row: at.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(workspace.handle(&event).permission.is_none());
        if drawer {
            let panel = workspace
                .state
                .drawer()
                .and_then(crate::state::Drawer::permissions)
                .expect("panel");
            assert_eq!(panel.choices()[panel.selected()].0, choice);
        } else {
            assert_eq!(
                workspace.state.menu_chosen(),
                Some(MenuRow::Permission(choice))
            );
        }
        assert!(
            workspace.handle(&key(KeyCode::Enter)).permission.is_none(),
            "review grants nothing"
        );
        workspace.settled_draw(&mut terminal).expect("confirmation");
        let text = region_text(
            terminal.backend().buffer(),
            workspace.surfaces.get(surface).expect("surface").bounds,
        );
        assert!(text.contains("> Back"), "{text}");
        assert!(
            workspace.handle(&key(KeyCode::Enter)).permission.is_none(),
            "Back applies no change"
        );
    }
}
