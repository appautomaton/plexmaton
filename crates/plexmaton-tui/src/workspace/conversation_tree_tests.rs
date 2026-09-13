//! The full-screen native tree, its responsive frame and stable navigation targets (TRE-1/TRE-6).

use std::time::{Duration, Instant};

use plexmaton_core::{
    AgentId, AgentStatus, ConversationEntryId, ConversationEvent, ConversationEventEnvelope,
    ConversationId, EventSequence, HeadName, TreeEditAction, TreeHead, TreeLabel, TreeNavigation,
    TreeNavigationTarget, TreeOrigin, TreePreview, TreeRevision, TreeRewindEligibility, TreeRow,
    TreeRowKind, TreeSnapshot, TreeSourceRequest,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
};

use super::Workspace;
use crate::{Flow, Point, SurfaceId, TreeRequest, test_support::region_text};

fn id(value: &str) -> ConversationEntryId {
    ConversationEntryId::new(value).expect("entry id")
}

fn branch(value: &str) -> HeadName {
    HeadName::new(value).expect("branch name")
}

fn snapshot() -> TreeSnapshot {
    let agent = AgentId::new("primary").expect("agent id");
    let main = branch("main");
    let old = branch("old");
    let first = id("user-1");
    let answer = id("assistant-2");
    let current = id("user-3");
    TreeSnapshot {
        origin: TreeOrigin {
            conversation_id: ConversationId::new("conversation").expect("conversation id"),
            agent_id: agent,
            selected_head: main.clone(),
            revision: TreeRevision::new(9),
        },
        heads: vec![
            TreeHead {
                name: main.clone(),
                target: Some(current.clone()),
            },
            TreeHead {
                name: old.clone(),
                target: Some(answer.clone()),
            },
        ],
        rows: vec![
            TreeRow {
                entry_id: first.clone(),
                parent_id: None,
                chronological_ordinal: 0,
                kind: TreeRowKind::User,
                preview: TreePreview {
                    text: "first question".to_owned(),
                    truncated: false,
                },
                label: None,
                head_markers: Vec::new(),
                active_ancestry: true,
                rewind: TreeRewindEligibility::Eligible,
            },
            TreeRow {
                entry_id: answer.clone(),
                parent_id: Some(first),
                chronological_ordinal: 1,
                kind: TreeRowKind::Assistant,
                preview: TreePreview {
                    text: "first answer".to_owned(),
                    truncated: false,
                },
                label: None,
                head_markers: vec![old],
                active_ancestry: true,
                rewind: TreeRewindEligibility::Eligible,
            },
            TreeRow {
                entry_id: current.clone(),
                parent_id: Some(answer),
                chronological_ordinal: 2,
                kind: TreeRowKind::User,
                preview: TreePreview {
                    text: "current question".to_owned(),
                    truncated: false,
                },
                label: None,
                head_markers: vec![main],
                active_ancestry: true,
                rewind: TreeRewindEligibility::Eligible,
            },
        ],
    }
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn chord(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn mouse(kind: MouseEventKind, at: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: at.x,
        row: at.y,
        modifiers: KeyModifiers::NONE,
    })
}

fn open_tree(width: u16, height: u16, tree: TreeSnapshot) -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::default();
    workspace.show_conversation_tree(tree.origin.agent_id.clone(), Ok(tree));
    workspace.set_working_directory("/project".to_owned());
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    workspace
        .settled_draw(&mut terminal)
        .expect("conversation-tree frame");
    (workspace, terminal)
}

fn live_workspace(width: u16, height: u16) -> (Workspace, Terminal<TestBackend>, AgentId) {
    let mut workspace = Workspace::default();
    workspace.emit(vec![ConversationEventEnvelope {
        sequence: EventSequence::new(1),
        event: ConversationEvent::AgentCreated {
            agent_id: snapshot().origin.agent_id,
            label: "Plexmaton".to_owned(),
            status: AgentStatus::Idle,
        },
    }]);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    workspace
        .settled_draw(&mut terminal)
        .expect("initial workspace frame");
    let agent = workspace
        .state
        .primary_agent()
        .expect("fixture primary agent")
        .id
        .clone();
    workspace
        .state
        .focus_surface(&workspace.surfaces, SurfaceId::Composer);
    (workspace, terminal, agent)
}

fn live_tree(width: u16, height: u16) -> (Workspace, Terminal<TestBackend>, AgentId) {
    let (mut workspace, mut terminal, agent) = live_workspace(width, height);
    workspace.show_conversation_tree(agent.clone(), Ok(snapshot()));
    workspace
        .settled_draw(&mut terminal)
        .expect("conversation-tree frame");
    (workspace, terminal, agent)
}

fn row_point(workspace: &Workspace, terminal: &Terminal<TestBackend>, signature: &str) -> Point {
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::ConversationTree)
        .expect("tree surface")
        .bounds;
    let drawn = region_text(terminal.backend().buffer(), bounds);
    let (row, _) = drawn
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(signature))
        .unwrap_or_else(|| panic!("{signature:?} is painted in the tree:\n{drawn}"));
    Point {
        x: bounds.x + 15,
        y: bounds.y + u16::try_from(row).expect("row offset"),
    }
}

/// TRE-1: wide, medium, narrow, the supported minimum, and too-small frames share one modal and
/// preserve the status/quit row; the medium footer selects a complete shorter hint.
#[test]
fn tre_1_tree_frames_keep_branch_copy_and_status_at_each_breakpoint() {
    for (width, height) in [(120, 30), (88, 30), (60, 30), (60, 12), (48, 12)] {
        let (workspace, terminal) = open_tree(width, height, snapshot());
        let tree = workspace
            .surfaces()
            .get(SurfaceId::ConversationTree)
            .expect("supported tree surface");
        let status = workspace
            .surfaces()
            .get(SurfaceId::Status)
            .expect("status remains registered below modal");
        assert_eq!(tree.bounds.width, width, "{width}x{height}");
        assert_eq!(tree.bounds.bottom(), status.bounds.y, "{width}x{height}");
        assert_eq!(status.bounds.height, 1, "{width}x{height}");
        let tree_text = region_text(terminal.backend().buffer(), tree.bounds);
        let status_text = region_text(terminal.backend().buffer(), status.bounds);
        for signature in [
            "Conversation tree",
            "Messages · active branch main · 2 branches",
            "b branches",
            "Esc/× close",
        ] {
            assert!(
                tree_text.contains(signature),
                "{width}x{height} lacks {signature:?}:\n{tree_text}"
            );
        }
        assert!(status_text.contains("/project"), "{width}x{height}");
        assert!(
            !tree_text.contains("Esc/× c…"),
            "{width}x{height} never truncates the close hint:\n{tree_text}"
        );
    }

    let (workspace, terminal) = open_tree(47, 12, snapshot());
    assert!(
        workspace.surfaces().is_empty(),
        "below minimum has no guessed hit tree"
    );
    let screen = region_text(
        terminal.backend().buffer(),
        terminal.backend().buffer().area,
    );
    assert!(screen.contains("Terminal too small"), "{screen}");
}

/// TRE-6/TRE-8: clicking selects a semantic identity without activating it; Enter is the explicit
/// navigation, branch selection stays separate from rewinding, and folds retain visible selection.
#[test]
fn tre_6_pointer_and_keyboard_navigation_use_stable_rows_and_branches() {
    let (mut workspace, terminal) = open_tree(120, 30, snapshot());
    let tree_bounds = workspace
        .surfaces()
        .get(SurfaceId::ConversationTree)
        .expect("tree surface")
        .bounds;
    let drawn = region_text(terminal.backend().buffer(), tree_bounds);
    let (row, _) = drawn
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("first answer"))
        .expect("semantic assistant row is painted");
    let at = Point {
        x: tree_bounds.x + 15,
        y: tree_bounds.y + u16::try_from(row).expect("row"),
    };
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
    let click = workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), at));
    assert!(
        click.tree.is_none(),
        "a click selects but does not navigate"
    );
    assert_eq!(
        workspace
            .state
            .tree()
            .and_then(|tree| tree.selected_entry()),
        Some(&id("assistant-2"))
    );

    let rewind = workspace.handle(&key(KeyCode::Enter));
    assert_eq!(
        rewind.tree,
        Some(TreeRequest::Navigate(TreeNavigation {
            origin: snapshot().origin,
            target: TreeNavigationTarget::Rewind(id("assistant-2")),
        }))
    );
    assert_eq!(rewind.flow, Flow::Continue);

    workspace.handle(&key(KeyCode::Home));
    workspace.handle(&key(KeyCode::Char('f')));
    let tree = workspace.state.tree().expect("retained tree");
    assert_eq!(tree.selected_entry(), Some(&id("user-1")));
    assert_eq!(tree.visible_entries().len(), 1, "fold hides descendants");

    workspace.handle(&key(KeyCode::Char('b')));
    workspace.handle(&key(KeyCode::End));
    let selected = workspace.state.tree().expect("branch view").selected_head();
    assert_eq!(selected.map(HeadName::as_str), Some("old"));
    let branch_selection = workspace.handle(&key(KeyCode::Enter));
    assert_eq!(
        branch_selection.tree,
        Some(TreeRequest::Navigate(TreeNavigation {
            origin: snapshot().origin,
            target: TreeNavigationTarget::SelectHead(branch("old")),
        }))
    );
}

/// TRE-1/TRE-4: Escape closes a tree even if too-small geometry registered no surface, and a
/// pending navigation remains pending rather than being represented as rolled back.
#[test]
fn tre_1_small_frame_escape_closes_but_does_not_cancel_an_admitted_write() {
    let (mut workspace, _terminal) = open_tree(47, 12, snapshot());
    workspace.mark_tree_navigation_pending();
    let outcome = workspace.handle(&key(KeyCode::Esc));
    assert!(outcome.tree.is_none());
    assert_eq!(outcome.flow, Flow::Continue);
    assert!(!workspace.state.conversation_tree_open());
    assert!(workspace.state.tree_navigation_pending());

    let reopened = workspace.handle(&key(KeyCode::Char('/')));
    assert!(
        reopened.tree.is_none(),
        "hidden composer shortcuts stay blocked"
    );
    assert!(!workspace.state.conversation_tree_open());
}

/// CMC-1/CMC-2/TRE-1: the rewind alias completes like the tree command but does not execute on Tab.
#[test]
fn tre_1_rewind_alias_completion_inserts_a_command_without_running_it() {
    let (mut workspace, mut terminal, _agent) = live_workspace(88, 30);
    let pasted = workspace.handle(&Event::Paste("/rewind".to_owned()));
    assert!(pasted.command.is_none() && pasted.tree.is_none());
    workspace
        .settled_draw(&mut terminal)
        .expect("alias completion frame");

    let menu = workspace
        .surfaces()
        .get(SurfaceId::ComposerMenu)
        .expect("the typed alias opens the command menu")
        .bounds;
    assert!(region_text(terminal.backend().buffer(), menu).contains("/rewind"));
    let completed = workspace.handle(&key(KeyCode::Tab));
    assert!(completed.command.is_none() && completed.tree.is_none());
    assert_eq!(workspace.state.composer().text(), "/rewind ");
    assert!(!workspace.state.conversation_tree_open());
}

/// TRE-1/INV-2/INV-7: text, interrupt, inspector and selection shortcuts cannot reach the covered
/// composer while its exact text and deliberate skill binding remain intact.
#[test]
fn tre_1_hidden_input_and_selection_shortcuts_preserve_exact_draft_and_skill() {
    let (mut workspace, _terminal, agent) = live_tree(88, 30);
    let draft = "$research keep this exact draft 界";
    workspace.return_skill_input(agent.clone(), draft.to_owned(), Some("research".to_owned()));
    // This helper opens the tree before the draft is seeded, so the draft is still the hidden
    // composer's exact source, not a reconstituted approximation.
    for event in [
        Event::Paste("must not replace it".to_owned()),
        chord(KeyCode::Char('c'), KeyModifiers::CONTROL),
        chord(KeyCode::Up, KeyModifiers::ALT),
        chord(KeyCode::Up, KeyModifiers::SHIFT),
        chord(KeyCode::Char('o'), KeyModifiers::CONTROL),
    ] {
        let outcome = workspace.handle(&event);
        assert!(outcome.submitted.is_none());
        assert!(outcome.tree.is_none());
    }
    assert_eq!(workspace.state.composer().text(), draft);
    assert_eq!(workspace.state.selected_skill(&agent), Some("research"));
    assert!(workspace.state.selection().is_none());
}

/// TRE-1/INV-7: Ctrl-D remains available over the tree; Ctrl-C withdraws only the pending quit
/// question, and the next deliberate pair still needs two presses.
#[test]
fn tre_1_global_quit_confirmation_rearms_after_interrupt_under_tree() {
    let (mut workspace, _terminal, _agent) = live_tree(88, 30);
    let start = Instant::now();
    let quit = chord(KeyCode::Char('d'), KeyModifiers::CONTROL);
    let interrupt = chord(KeyCode::Char('c'), KeyModifiers::CONTROL);

    assert_eq!(workspace.handle_at(&quit, start).flow, Flow::Continue);
    assert_eq!(
        workspace.state.status().note(),
        crate::state::StatusNote::QuitArmed {
            deadline: start + Duration::from_secs(1),
        }
    );
    assert_eq!(
        workspace
            .handle_at(&interrupt, start + Duration::from_millis(100))
            .flow,
        Flow::Continue
    );
    assert_eq!(
        workspace.state.status().note(),
        crate::state::StatusNote::Quiet
    );
    assert_eq!(
        workspace
            .handle_at(&quit, start + Duration::from_millis(200))
            .flow,
        Flow::Continue
    );
    assert_eq!(
        workspace
            .handle_at(&quit, start + Duration::from_millis(300))
            .flow,
        Flow::Quit
    );
    assert!(workspace.state.conversation_tree_open());
}

/// TRE-6/INV-3: an actual pointer hover selects the same semantic row, arrows take over, and a
/// repeated stationary mouse sample cannot reclaim the cursor from the keyboard.
#[test]
fn tre_6_stationary_pointer_repeat_does_not_override_keyboard_cursor() {
    let (mut workspace, terminal, _agent) = live_tree(120, 30);
    let hovered = row_point(&workspace, &terminal, "first answer");
    workspace.handle(&mouse(MouseEventKind::Moved, hovered));
    assert_eq!(
        workspace
            .state
            .tree()
            .and_then(|tree| tree.selected_entry()),
        Some(&id("assistant-2"))
    );

    workspace.handle(&key(KeyCode::Up));
    assert_eq!(
        workspace
            .state
            .tree()
            .and_then(|tree| tree.selected_entry()),
        Some(&id("user-1"))
    );
    workspace.handle(&mouse(MouseEventKind::Moved, hovered));
    assert_eq!(
        workspace
            .state
            .tree()
            .and_then(|tree| tree.selected_entry()),
        Some(&id("user-1"))
    );
}

/// TRE-6: branch rows participate in pointer selection, but Enter still emits the distinct
/// SelectHead navigation target rather than a message rewind.
#[test]
fn tre_6_pointer_selects_a_branch_before_explicit_head_navigation() {
    let (mut workspace, mut terminal, _agent) = live_tree(120, 30);
    workspace.handle(&key(KeyCode::Char('b')));
    workspace
        .settled_draw(&mut terminal)
        .expect("branch-list frame");
    let old = row_point(&workspace, &terminal, "old  ");
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), old));
    let click = workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), old));
    assert!(click.tree.is_none(), "pointer selection is not activation");
    assert_eq!(
        workspace
            .state
            .tree()
            .and_then(|tree| tree.selected_head())
            .map(HeadName::as_str),
        Some("old")
    );

    assert_eq!(
        workspace.handle(&key(KeyCode::Enter)).tree,
        Some(TreeRequest::Navigate(TreeNavigation {
            origin: snapshot().origin,
            target: TreeNavigationTarget::SelectHead(branch("old")),
        }))
    );
}

/// TRE-8: both advertised copy keys emit an exact stable-source request, not painted row text.
#[test]
fn tre_8_y_and_ctrl_y_return_the_same_workspace_copy_request() {
    let (mut workspace, _terminal, _agent) = live_tree(120, 30);
    let expected = TreeRequest::Copy(TreeSourceRequest {
        origin: snapshot().origin,
        entry_id: id("user-3"),
    });
    assert_eq!(
        workspace.handle(&key(KeyCode::Char('y'))).tree,
        Some(expected.clone())
    );
    assert_eq!(
        workspace
            .handle(&chord(KeyCode::Char('y'), KeyModifiers::CONTROL))
            .tree,
        Some(expected)
    );
}

/// TRE-6/INV-11: once a row gesture moves, is resized, or is escaped, its later release is inert.
#[test]
fn tre_6_tree_press_drag_resize_and_escape_never_release_activate() {
    for cancellation in ["drag", "resize", "escape"] {
        let (mut workspace, terminal, _agent) = live_tree(120, 30);
        let at = row_point(&workspace, &terminal, "first answer");
        let before = workspace
            .state
            .tree()
            .and_then(|tree| tree.selected_entry())
            .cloned();
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at));
        match cancellation {
            "drag" => {
                let other = row_point(&workspace, &terminal, "first question");
                workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), other));
            }
            "resize" => {
                workspace.handle(&Event::Resize(88, 30));
                let mut resized =
                    Terminal::new(TestBackend::new(88, 30)).expect("resized terminal");
                workspace
                    .settled_draw(&mut resized)
                    .expect("repaint resized tree");
            }
            "escape" => {
                workspace.handle(&key(KeyCode::Esc));
            }
            _ => unreachable!("fixed cancellation cases"),
        }
        let released = workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), at));
        assert!(
            released.tree.is_none(),
            "{cancellation} cannot activate a row"
        );
        assert!(workspace.state.conversation_tree_open());
        assert_eq!(
            workspace
                .state
                .tree()
                .and_then(|tree| tree.selected_entry())
                .cloned(),
            before,
            "{cancellation} leaves the cursor on its prior identity"
        );
    }
}

/// TRE-8/SURF-3: a label editor owns the only caret, accepts paste, and Escape cancels the editor
/// before a second Escape closes browsing.
#[test]
fn tre_8_label_editor_paints_its_caret_and_escape_returns_to_browsing() {
    let (mut workspace, mut terminal, _agent) = live_tree(120, 30);
    workspace.handle(&key(KeyCode::Char('l')));
    workspace
        .settled_draw(&mut terminal)
        .expect("label editor frame");
    let tree = workspace
        .surfaces()
        .get(SurfaceId::ConversationTree)
        .expect("tree surface")
        .bounds;
    assert!(
        terminal.backend().cursor_visible(),
        "tree input owns the caret"
    );
    let caret = terminal.backend().cursor_position();
    assert!(caret.x >= tree.x && caret.x < tree.right());
    assert!(caret.y >= tree.y && caret.y < tree.bottom() - 1);

    workspace.handle(&Event::Paste("release checklist".to_owned()));
    workspace
        .settled_draw(&mut terminal)
        .expect("edited label frame");
    let editor = region_text(terminal.backend().buffer(), tree);
    assert!(
        editor.contains("Label message: release checklist"),
        "{editor}"
    );
    workspace.handle(&key(KeyCode::Esc));
    assert!(workspace.state.conversation_tree_open());
    assert!(!workspace.state.tree_editor_open());
    workspace.handle(&key(KeyCode::Esc));
    assert!(!workspace.state.conversation_tree_open());
}

/// TRE-4/TRE-8: Enter admits one metadata write; duplicates are ignored, and Escape closes only
/// its view while the acknowledged-writer pending state remains owned until its callback.
#[test]
fn tre_4_duplicate_metadata_enter_and_close_do_not_cancel_an_admitted_write() {
    let (mut workspace, _terminal, agent) = live_tree(120, 30);
    workspace.handle(&key(KeyCode::Char('l')));
    workspace.handle(&Event::Paste("release checklist".to_owned()));
    let admitted = workspace.handle(&key(KeyCode::Enter));
    let expected = TreeRequest::Edit(plexmaton_core::TreeEdit {
        origin: snapshot().origin,
        action: TreeEditAction::SetLabel {
            entry_id: id("user-3"),
            label: Some(TreeLabel::new("release checklist".to_owned()).expect("valid label")),
        },
    });
    assert_eq!(admitted.tree, Some(expected));
    workspace.mark_tree_edit_pending();

    assert!(workspace.handle(&key(KeyCode::Enter)).tree.is_none());
    assert!(workspace.state.tree_navigation_pending());
    workspace.handle(&key(KeyCode::Esc));
    assert!(!workspace.state.conversation_tree_open());
    assert!(workspace.state.tree_navigation_pending());

    workspace.complete_tree_edit(&agent, Ok(snapshot()));
    assert!(!workspace.state.conversation_tree_open());
    assert!(!workspace.state.tree_navigation_pending());
}

/// TRE-1/SURF-4: a Drawer layered over the tree keeps its dismissal rung through a geometry where
/// neither modal has a visible surface, then the tree reclaims focus when the frame returns.
#[test]
fn tre_1_minimum_size_escape_closes_drawer_before_tree_and_blocks_hidden_input() {
    let (mut workspace, mut terminal, agent) = live_workspace(88, 30);
    let draft = "$research exact hidden draft";
    workspace.return_skill_input(agent.clone(), draft.to_owned(), Some("research".to_owned()));
    workspace.show_conversation_tree(agent.clone(), Ok(snapshot()));
    workspace.settled_draw(&mut terminal).expect("tree frame");
    assert_eq!(
        workspace.state.focused(workspace.surfaces()),
        Some(SurfaceId::ConversationTree)
    );
    workspace.handle(&chord(KeyCode::Char('p'), KeyModifiers::CONTROL));
    workspace
        .settled_draw(&mut terminal)
        .expect("drawer over tree");
    assert!(workspace.state.drawer().is_some());
    assert_eq!(
        workspace.state.drawer().map(|drawer| drawer.return_focus()),
        Some(SurfaceId::ConversationTree)
    );

    workspace.handle(&Event::Resize(47, 12));
    let mut narrow = Terminal::new(TestBackend::new(47, 12)).expect("minimum terminal");
    workspace.settled_draw(&mut narrow).expect("minimum frame");
    assert!(
        workspace
            .surfaces()
            .get(SurfaceId::ConversationTree)
            .is_none()
    );
    workspace.handle(&key(KeyCode::Esc));
    assert!(workspace.state.drawer().is_none());
    assert!(workspace.state.conversation_tree_open());
    workspace.handle(&Event::Paste("paste cannot reach composer".to_owned()));
    workspace.handle(&key(KeyCode::Char('x')));
    assert_eq!(workspace.state.composer().text(), draft);
    assert_eq!(workspace.state.selected_skill(&agent), Some("research"));

    workspace.handle(&Event::Resize(88, 30));
    let mut restored = Terminal::new(TestBackend::new(88, 30)).expect("restored terminal");
    workspace
        .settled_draw(&mut restored)
        .expect("restored tree frame");
    assert!(workspace.state.drawer().is_none());
    assert!(workspace.state.conversation_tree_open());
    assert_eq!(
        workspace.state.focused(workspace.surfaces()),
        Some(SurfaceId::ConversationTree)
    );
    workspace.handle(&Event::Paste("still hidden".to_owned()));
    workspace.handle(&key(KeyCode::Char('x')));
    assert_eq!(workspace.state.composer().text(), draft);
    assert_eq!(workspace.state.selected_skill(&agent), Some("research"));
}
