use super::*;
use crate::{restore_undelivered, test_support::empty_session, tests::FixtureWorkspace};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use plexmaton_agent::{JournalSequence, ReturnedDraft, TreeEditResult, TreeNavigationResult};
use plexmaton_runtime::{DispatchReport, PersistenceFailure};
use ratatui::{Terminal, backend::TestBackend};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn focus_composer(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) {
    workspace.draw(terminal).expect("frame");
    for _ in 0..workspace.surfaces().len() {
        if workspace.state().focused(workspace.surfaces())
            == Some(plexmaton_tui::SurfaceId::Composer)
        {
            return;
        }
        workspace.handle(&key(KeyCode::Tab));
        workspace.draw(terminal).expect("focus frame");
    }
    panic!("composer must be a reachable focus stop");
}

/// TRE-4/TRE-5: the real report adapter replaces projection before delivering the exact historical
/// text/skill pair. Resetting state must not erase the pending tree's return-focus/draft ownership.
#[tokio::test]
async fn tre_4_5_report_restores_draft_only_after_ack_and_preserves_occupied_input() {
    for existing in ["", "my unsent question"] {
        let root = FixtureWorkspace::new();
        let (mut runtime, mut picker, mut workspace, _) = empty_session(root.path());
        let agent = runtime.agent_id().clone();
        let configuration = picker.configuration();
        let effort = configuration.reasoning_effort;
        workspace.set_model(configuration);
        let mut terminal = Terminal::new(TestBackend::new(88, 30)).expect("terminal");
        focus_composer(&mut workspace, &mut terminal);
        if !existing.is_empty() {
            workspace.handle(&Event::Paste(existing.to_owned()));
        }
        let (journal, selected) = runtime.acknowledged_conversation().expect("acknowledged");
        let projection = journal
            .project(selected)
            .expect("projection")
            .events()
            .to_vec();
        let selected = selected.clone();
        open(&runtime, &mut workspace, &agent);
        workspace.mark_tree_navigation_pending();
        assert_eq!(workspace.state().composer().text(), existing);
        restore_undelivered(
            &mut workspace,
            agent.clone(),
            DispatchReport {
                projection_reset: Some(projection),
                tree_navigation: Some(TreeNavigationResult {
                    selected_head: selected,
                    mutation_sequence: Some(JournalSequence::new(1)),
                    returned_draft: Some(ReturnedDraft {
                        text: "$100 exact 中文\n  spacing".to_owned(),
                        skill_name: Some("100".to_owned()),
                    }),
                }),
                ..DispatchReport::default()
            },
        );
        assert_eq!(
            workspace.state().composer().text(),
            if existing.is_empty() {
                "$100 exact 中文\n  spacing"
            } else {
                existing
            }
        );
        workspace.draw(&mut terminal).expect("acknowledged frame");
        let composer = workspace
            .surfaces()
            .get(plexmaton_tui::SurfaceId::Composer)
            .expect("composer")
            .bounds;
        let header: String = (composer.x..composer.right())
            .map(|x| terminal.backend().buffer()[(x, composer.y)].symbol())
            .collect();
        assert!(
            header.contains(effort.as_str()),
            "resolved effort vanished: {header}"
        );
        let submitted = workspace
            .handle(&key(KeyCode::Enter))
            .submitted
            .expect("separate send");
        assert_eq!(
            submitted.skill.as_deref(),
            if existing.is_empty() {
                Some("100")
            } else {
                None
            }
        );
        assert!(
            !runtime.has_active_work(),
            "the adapter must not dispatch the restored draft"
        );
        picker.shutdown().await.expect("picker");
        runtime.shutdown().await.expect("runtime");
    }
}

/// TRE-4/TRE-5: persistence failure is diagnostic only, never a successful completion or a draft
/// return. Both definite and uncertain failure preserve the caller-owned input.
#[tokio::test]
async fn tre_4_5_failed_report_keeps_pending_tree_input() {
    for failure in [
        PersistenceFailure::NotWritten,
        PersistenceFailure::OutcomeUnknown,
    ] {
        let root = FixtureWorkspace::new();
        let (mut runtime, mut picker, mut workspace, _) = empty_session(root.path());
        let agent = runtime.agent_id().clone();
        workspace.return_skill_input(agent.clone(), "keep exact draft".to_owned(), None);
        open(&runtime, &mut workspace, &agent);
        workspace.mark_tree_navigation_pending();
        restore_undelivered(
            &mut workspace,
            agent,
            DispatchReport {
                persistence_failure: Some(failure),
                ..DispatchReport::default()
            },
        );
        assert_eq!(workspace.state().composer().text(), "keep exact draft");
        assert!(!runtime.has_active_work());
        picker.shutdown().await.expect("picker");
        runtime.shutdown().await.expect("runtime");
    }
}

/// TRE-4/TRE-5/INV-6: an actual navigation report replaces the transcript without dropping a
/// Drawer opened while the write was pending; dismissing it returns to the surviving composer.
#[tokio::test]
async fn tre_4_5_report_preserves_drawer_and_rebases_its_return_focus() {
    let root = FixtureWorkspace::new();
    let (mut runtime, mut picker, mut workspace, _) = empty_session(root.path());
    let agent = runtime.agent_id().clone();
    let mut terminal = Terminal::new(TestBackend::new(88, 30)).expect("terminal");
    focus_composer(&mut workspace, &mut terminal);
    let (journal, selected) = runtime.acknowledged_conversation().expect("acknowledged");
    let projection = journal
        .project(selected)
        .expect("projection")
        .events()
        .to_vec();
    let selected = selected.clone();
    open(&runtime, &mut workspace, &agent);
    workspace.mark_tree_navigation_pending();
    workspace.draw(&mut terminal).expect("pending frame");
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )));
    workspace.draw(&mut terminal).expect("Drawer frame");
    assert_eq!(
        workspace.state().focused(workspace.surfaces()),
        Some(plexmaton_tui::SurfaceId::Drawer)
    );
    restore_undelivered(
        &mut workspace,
        agent,
        DispatchReport {
            projection_reset: Some(projection),
            tree_navigation: Some(TreeNavigationResult {
                selected_head: selected,
                mutation_sequence: Some(JournalSequence::new(1)),
                returned_draft: Some(ReturnedDraft {
                    text: "restored question".to_owned(),
                    skill_name: None,
                }),
            }),
            ..DispatchReport::default()
        },
    );
    workspace.draw(&mut terminal).expect("ack frame");
    assert_eq!(
        workspace.state().focused(workspace.surfaces()),
        Some(plexmaton_tui::SurfaceId::Drawer)
    );
    assert!(
        workspace
            .surfaces()
            .get(plexmaton_tui::SurfaceId::ConversationTree)
            .is_none()
    );
    assert_eq!(workspace.state().composer().text(), "restored question");
    workspace.handle(&key(KeyCode::Esc));
    workspace.draw(&mut terminal).expect("return frame");
    assert_eq!(
        workspace.state().focused(workspace.surfaces()),
        Some(plexmaton_tui::SurfaceId::Composer)
    );
    picker.shutdown().await.expect("picker");
    runtime.shutdown().await.expect("runtime");
}

/// TRE-4/TRE-8: the adapter refreshes metadata from the acknowledged runtime, not the old UI
/// snapshot; an acknowledged edit keeps browsing open but never reopens a deliberately closed view.
#[tokio::test]
async fn tre_4_8_edit_report_refreshes_acknowledged_metadata_without_reopening() {
    for close_before_ack in [false, true] {
        let root = FixtureWorkspace::new();
        let (mut runtime, mut picker, mut workspace, _) = empty_session(root.path());
        let agent = runtime.agent_id().clone();
        let origin = runtime.acknowledged_tree_origin().expect("origin");
        let mut previous = runtime
            .acknowledged_conversation()
            .expect("journal")
            .0
            .tree_snapshot(&agent)
            .expect("snapshot");
        // The adapter receives snapshots, not journal mutations. Present a prior branch name;
        // the report must replace it with the actual runtime snapshot's current name.
        let previous_name = plexmaton_core::HeadName::new("before-rename").expect("name");
        previous.origin.selected_head = previous_name.clone();
        previous.heads[0].name = previous_name;
        workspace.show_conversation_tree(agent.clone(), Ok(previous));
        workspace.mark_tree_edit_pending();
        let mut terminal = Terminal::new(TestBackend::new(88, 30)).expect("terminal");
        workspace.draw(&mut terminal).expect("pending frame");
        if close_before_ack {
            workspace.handle(&key(KeyCode::Esc));
        }
        crate::input::apply_report(
            &runtime,
            &mut workspace,
            agent,
            DispatchReport {
                tree_edit: Some(TreeEditResult {
                    origin,
                    mutation_sequence: Some(JournalSequence::new(1)),
                }),
                ..DispatchReport::default()
            },
        );
        workspace.draw(&mut terminal).expect("ack frame");
        let tree_visible = workspace
            .surfaces()
            .get(plexmaton_tui::SurfaceId::ConversationTree)
            .is_some();
        assert_eq!(tree_visible, !close_before_ack);
        let frame: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(!frame.contains("before-rename"));
        assert!(!frame.contains("Saving history"));
        if !close_before_ack {
            assert!(frame.contains("active branch main"), "{frame}");
        }
        assert!(workspace.state().composer().text().is_empty());
        assert!(!runtime.has_active_work());
        picker.shutdown().await.expect("picker");
        runtime.shutdown().await.expect("runtime");
    }
}
