use super::*;
use crate::SurfaceId;
use plexmaton_core::{
    CodingSessionId, NativeFilePreset, PermissionAction, PermissionChangeError, PermissionRevision,
    PermissionStateView,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
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
fn panel(workspace: &Workspace, terminal: &Terminal<TestBackend>) -> String {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::CommandPalette)
        .expect("permissions")
        .bounds;
    crate::test_support::snapshot_text(terminal.backend().buffer(), bounds)
}
fn click(workspace: &mut Workspace, terminal: &Terminal<TestBackend>, text: &str) -> Outcome {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::CommandPalette)
        .expect("permissions")
        .bounds;
    let drawn = panel(workspace, terminal);
    let (row, line) = drawn
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(text))
        .expect("visible choice");
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

/// PER-7/SURF-3: keyboard and drawn-row clicks emit one reviewed revision; the producer owns completion.
#[test]
fn per_7_permission_controls_review_cancel_submit_and_refresh_by_identity() {
    for width in [120, 95, 60, 48] {
        for pointer in [false, true] {
            let mut workspace = Workspace::default();
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
            workspace.open_permissions();
            workspace.update_permissions(Ok(view()), None);
            workspace.draw(&mut terminal).expect("browse");
            assert_eq!(
                workspace.state().keyboard_focus(workspace.surfaces()),
                crate::KeyboardFocus::Navigation
            );
            workspace.handle(&Event::Paste("not a filter".into()));
            assert!(
                workspace
                    .state()
                    .command_palette()
                    .expect("page")
                    .filter()
                    .text()
                    .is_empty()
            );
            let review = if pointer {
                click(&mut workspace, &terminal, "Enable native")
            } else {
                workspace.handle(&key(KeyCode::Enter))
            };
            assert!(review.permission.is_none(), "opening review grants nothing");
            workspace.draw(&mut terminal).expect("confirmation");
            assert!(panel(&workspace, &terminal).contains("> Back"));
            workspace.handle(&key(KeyCode::Esc));
            workspace.draw(&mut terminal).expect("back");
            assert!(panel(&workspace, &terminal).contains("Enable native"));
            workspace.handle(&key(KeyCode::Enter));
            workspace.handle(&key(KeyCode::Up));
            workspace.draw(&mut terminal).expect("chosen confirmation");
            let confirmed = if pointer {
                click(&mut workspace, &terminal, "> Enable for")
            } else {
                workspace.handle(&key(KeyCode::Enter))
            };
            let intent = confirmed.permission.expect("explicit confirmation");
            assert_eq!(intent.expected, view().revision);
            assert_eq!(intent.action, PermissionAction::EnableNativeFiles);
            assert!(workspace.handle(&key(KeyCode::Enter)).permission.is_none());
            workspace.draw(&mut terminal).expect("submitting");
            assert!(panel(&workspace, &terminal).contains("Applying change"));
            let mut fresh = view();
            fresh.revision = fresh.revision.next().expect("new revision");
            workspace.update_permissions(
                Ok(fresh.clone()),
                Some(Err(PermissionChangeError::StaleRevision)),
            );
            workspace.draw(&mut terminal).expect("refused");
            assert!(panel(&workspace, &terminal).contains("permissions changed"));
            workspace.handle(&key(KeyCode::Enter));
            workspace.handle(&key(KeyCode::Up));
            assert_eq!(
                workspace
                    .handle(&key(KeyCode::Enter))
                    .permission
                    .expect("new confirmation")
                    .expected,
                fresh.revision
            );
            workspace.handle(&key(KeyCode::Esc));
            workspace.update_permissions(Ok(view()), Some(Ok(())));
            assert!(
                !workspace.permissions_open(),
                "late completion cannot reopen a dismissed page"
            );
        }
    }
}

/// PER-7/INV-13: reviewable three-width frames and a smallest-terminal confirmation keep the actions visible.
#[test]
fn per_7_permission_controls_frames_keep_scope_and_confirmation_visible() {
    for (width, name) in [(120, "wide"), (95, "medium"), (60, "narrow")] {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
        workspace.open_permissions();
        workspace.update_permissions(Ok(view()), None);
        workspace.draw(&mut terminal).expect("browse");
        let mut drawn = panel(&workspace, &terminal);
        assert!(
            drawn.contains("Enter select") && drawn.contains("Esc close"),
            "{drawn}"
        );
        workspace.handle(&key(KeyCode::Enter));
        workspace.draw(&mut terminal).expect("review");
        let confirmation = panel(&workspace, &terminal);
        for text in [
            "create/edit",
            "configuration",
            "Git metadata",
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
    let mut workspace = Workspace::default();
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("terminal");
    workspace.open_permissions();
    workspace.update_permissions(Ok(view()), None);
    workspace.draw(&mut terminal).expect("small browse");
    workspace.handle(&key(KeyCode::Enter));
    workspace.draw(&mut terminal).expect("small confirm");
    let shown = panel(&workspace, &terminal);
    for text in ["create/edit", "Enable for this Session", "> Back"] {
        assert!(shown.contains(text), "{shown}");
    }
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
            .get(SurfaceId::CommandPalette)
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
                .viewport(SurfaceId::CommandPalette)
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
                        .get(SurfaceId::CommandPalette)
                        .expect("surface")
                        .bounds
                ),
                footer
            );
            let after = workspace
                .surfaces()
                .viewport(SurfaceId::CommandPalette)
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
        let intent = workspace
            .handle(&key(KeyCode::Enter))
            .permission
            .expect("explicit trust intent");
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

/// PER-8/INV-13: real three-width frames keep project lifetime, source, rules and explicit activation legible.
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
        let text = crate::test_support::snapshot_text(terminal.backend().buffer(), bounds);
        for required in [
            "exec_command",
            "Project permission saved",
            "tool did not run",
            "Review /permissions",
        ] {
            assert!(text.contains(required), "{name}: {required}: {text}");
        }
        crate::test_support::assert_frame(&format!("project-permission-receipt-{name}"), &text);
    }
}
