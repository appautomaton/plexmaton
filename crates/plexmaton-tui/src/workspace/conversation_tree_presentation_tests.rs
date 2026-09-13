//! TRE-1/TRE-2/TRE-6: real tree rendering for repeated steps and interleaved branches.
use super::*;

#[path = "../../examples/support/frame_svg.rs"]
mod frame_svg;

fn open_presentation_tree(
    width: u16,
    height: u16,
    tree: TreeSnapshot,
) -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::with_palette(crate::Palette::pastel());
    workspace.show_conversation_tree(tree.origin.agent_id.clone(), Ok(tree));
    workspace.set_working_directory("/project".into());
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    workspace
        .settled_draw(&mut terminal)
        .expect("native tree frame");
    (workspace, terminal)
}

fn presentation_snapshot(interleaved: bool) -> TreeSnapshot {
    let mut source = snapshot();
    let template = source.rows[0].clone();
    let definitions = [
        (
            "original",
            None,
            TreeRowKind::User,
            "请检查 conversation tree 的分支结构",
        ),
        (
            "tools-1",
            Some("original"),
            TreeRowKind::ToolBatch,
            "read_file read_file · Inspect the journal",
        ),
        (
            "tools-2",
            Some("tools-1"),
            TreeRowKind::ToolBatch,
            "read_file exec_command · Inspect navigation",
        ),
        (
            "tools-3",
            Some("tools-2"),
            TreeRowKind::ToolBatch,
            "search read_file · Find existing witnesses",
        ),
        (
            "tools-4",
            Some("tools-3"),
            TreeRowKind::ToolBatch,
            "exec_command · Check branch boundaries",
        ),
        (
            "answer",
            Some("tools-4"),
            TreeRowKind::Assistant,
            "历史结构完整；连续步骤不应逐层缩进。",
        ),
        (
            "alternate",
            None,
            TreeRowKind::User,
            "请检查 conversation tree 的分支结构",
        ),
        (
            "alternate-answer",
            Some("alternate"),
            TreeRowKind::Assistant,
            "另一条回答保留在独立分支中。",
        ),
    ];
    source.rows = definitions
        .into_iter()
        .enumerate()
        .map(|(index, (entry, parent, kind, text))| {
            let mut row = template.clone();
            row.entry_id = id(entry);
            row.parent_id = parent.map(id);
            row.chronological_ordinal = u64::try_from(index).expect("fixture ordinal");
            row.kind = kind;
            row.preview.text = text.into();
            row.head_markers.clear();
            row.active_ancestry = index < 6;
            row.rewind = if kind == TreeRowKind::ToolBatch {
                TreeRewindEligibility::Ineligible
            } else {
                TreeRewindEligibility::Eligible
            };
            row
        })
        .collect();
    source.rows[5].head_markers = vec![branch("main"), branch("rewind-109")];
    source.rows[7].head_markers = vec![branch("rewind-103")];
    source.origin.selected_head = branch("rewind-109");
    source.heads = vec![
        TreeHead {
            name: branch("main"),
            target: Some(id("answer")),
        },
        TreeHead {
            name: branch("rewind-103"),
            target: Some(id("alternate-answer")),
        },
        TreeHead {
            name: branch("rewind-109"),
            target: Some(id("answer")),
        },
    ];
    if interleaved {
        let mut later = source.rows[5].clone();
        later.entry_id = id("continued");
        later.parent_id = Some(id("answer"));
        later.kind = TreeRowKind::User;
        later.preview.text = "Continue the original branch after visiting the other one".into();
        later.chronological_ordinal = 8;
        later.active_ancestry = false;
        later.head_markers = vec![branch("main")];
        source.rows[5].head_markers = vec![branch("rewind-109")];
        source.heads[0].target = Some(later.entry_id.clone());
        source.rows.push(later);
    }
    source
}

/// TRE-1/TRE-2: shared tips stay visible beside long text and tool steps occupy one branch lane.
#[test]
fn tre_1_2_native_branch_frames_preserve_heads_and_continuation_lanes() {
    for width in [120, 88, 60] {
        for interleaved in [false, true] {
            let (workspace, terminal) =
                open_presentation_tree(width, 22, presentation_snapshot(interleaved));
            let bounds = workspace
                .surfaces()
                .get(SurfaceId::ConversationTree)
                .expect("tree")
                .bounds;
            let text = crate::test_support::snapshot_text(terminal.backend().buffer(), bounds);
            let lines = text.lines().collect::<Vec<_>>();
            let row = |needle: &str| {
                lines
                    .iter()
                    .position(|line| line.contains(needle))
                    .expect("fixture text visible")
            };
            assert!(lines[row("历史结构完整")].contains("● rewind-109"));
            if !interleaved {
                assert!(lines[row("历史结构完整")].contains("main"));
            } else {
                assert!(row("Continue the original") < row("另一条回答"), "{text}");
            }
            assert!(!text.contains(" tools "), "{text}");
            assert!(!text.contains("exec_command"), "{text}");
            assert!(text.contains("[−]"), "{text}");
            assert!(text.contains("├─"));
            assert!(text.contains("└─"));
            if std::env::var_os("PLEXMATON_WRITE_FRAMES").is_some() {
                let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("frames/conversation-tree");
                std::fs::create_dir_all(&root).expect("frame directory");
                let name = if interleaved {
                    "interleaved"
                } else {
                    "shared-heads"
                };
                std::fs::write(
                    root.join(format!("{name}-{width}.svg")),
                    frame_svg::svg(terminal.backend().buffer()),
                )
                .expect("frame write");
            }
        }
    }
}

/// TRE-6: the painted DFS row owns clicks, navigation and fold-hit geometry after a branch return.
#[test]
fn tre_6_interleaved_native_rows_keep_pointer_navigation_and_fold_identity() {
    let (mut workspace, terminal) = open_presentation_tree(88, 22, presentation_snapshot(true));
    let point = row_point(&workspace, &terminal, "Continue the original");
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
    workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), point));
    assert_eq!(
        workspace.state.tree().expect("tree").selected_entry(),
        Some(&id("continued"))
    );
    let request = workspace.handle(&key(KeyCode::Enter));
    assert!(
        matches!(request.tree, Some(TreeRequest::Navigate(TreeNavigation { target: TreeNavigationTarget::Rewind(ref entry), .. })) if entry == &id("continued"))
    );
    // Use a separate browsing session: the prior navigation intent remains owned until a receipt.
    let (mut workspace, mut terminal) = open_presentation_tree(88, 22, presentation_snapshot(true));
    let point = row_point(&workspace, &terminal, "● rewind-109");
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
    workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), point));
    workspace.handle(&key(KeyCode::Char('f')));
    workspace.settled_draw(&mut terminal).expect("folded draw");
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::ConversationTree)
        .expect("tree")
        .bounds;
    let text = crate::test_support::snapshot_text(terminal.backend().buffer(), bounds);
    assert!(!text.contains("Continue the original"));
    assert!(text.replace(' ', "").contains("另一条回答"), "{text}");
}

/// TRE-2/TRE-6: intermediate tools are not foldable navigation rows; answers remain reachable.
#[test]
fn tre_2_6_tool_chains_do_not_own_message_folding_or_selection() {
    let mut owned = crate::state::ConversationTree::new(
        AgentId::new("primary").expect("agent"),
        Ok(presentation_snapshot(false)),
        SurfaceId::Composer,
    );
    let tree = &mut owned;
    let ids = |tree: &crate::state::ConversationTree| {
        tree.visible_entries()
            .iter()
            .map(|row| row.entry_id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(tree),
        [
            id("original"),
            id("answer"),
            id("alternate"),
            id("alternate-answer")
        ]
    );
    for tool in ["tools-1", "tools-2", "tools-3", "tools-4"] {
        assert!(!tree.toggle_fold(&id(tool)));
        assert!(!tree.hover_entry(&id(tool), 20));
    }
    assert!(tree.toggle_fold(&id("original")));
    assert_eq!(
        ids(tree),
        [id("original"), id("alternate"), id("alternate-answer")]
    );
    assert_eq!(tree.selected_entry(), Some(&id("original")));
    assert!(tree.toggle_fold(&id("original")));
    assert!(ids(tree).contains(&id("answer")));
}

/// TRE-1/TRE-7: a read-only checkpoint has no rewind/retry offer or error loop on Enter.
#[test]
fn tre_1_7_read_only_selection_never_advertises_rewind_or_retry() {
    let mut source = snapshot();
    source.rows[2].kind = TreeRowKind::Checkpoint;
    source.rows[2].rewind = TreeRewindEligibility::Ineligible;
    let (mut workspace, mut terminal) = open_presentation_tree(88, 22, source);
    let text = crate::test_support::snapshot_text(
        terminal.backend().buffer(),
        terminal.backend().buffer().area,
    );
    assert!(!text.contains("rewind ·"), "{text}");
    assert!(text.contains("Read-only"), "{text}");
    assert!(workspace.handle(&key(KeyCode::Enter)).tree.is_none());
    workspace.settled_draw(&mut terminal).expect("draw");
    let text = crate::test_support::snapshot_text(
        terminal.backend().buffer(),
        terminal.backend().buffer().area,
    );
    assert!(!text.contains("retry"), "{text}");
    assert!(!text.contains("not a safe"), "{text}");
}

/// TRE-1/TRE-6: +/- controls retain their palette role, weight and full click target under selection.
#[test]
fn tre_1_6_disclosure_and_head_markers_keep_semantic_styles_when_selected() {
    use crate::theme::Role;
    let palette = crate::Palette::pastel();
    let (mut workspace, mut terminal) =
        open_presentation_tree(88, 22, presentation_snapshot(false));
    let locate = |buffer: &ratatui::buffer::Buffer, symbol: &str| {
        buffer
            .area
            .positions()
            .find(|position| buffer[*position].symbol() == symbol)
            .expect("painted symbol")
    };
    let minus = locate(terminal.backend().buffer(), "−");
    let before = terminal.backend().buffer()[minus].clone();
    assert_eq!(
        before.fg,
        palette.style(Role::Accent).fg.expect("accent foreground")
    );
    workspace.handle(&key(KeyCode::Home));
    workspace.settled_draw(&mut terminal).expect("selected row");
    let after = &terminal.backend().buffer()[minus];
    assert_eq!(before.fg, after.fg);
    assert_eq!(before.modifier, after.modifier);
    assert_eq!(
        after.bg,
        palette.style(Role::Chosen).bg.expect("chosen background")
    );
    let branch_line = locate(terminal.backend().buffer(), "├");
    assert_eq!(
        terminal.backend().buffer()[branch_line].fg,
        palette.style(Role::Muted).fg.expect("muted")
    );
    let head = locate(terminal.backend().buffer(), "●");
    assert_eq!(
        terminal.backend().buffer()[head].fg,
        palette.style(Role::Accent).fg.expect("accent")
    );
    // Click the closing bracket, not merely the center glyph: the entire control is one target.
    let point = Point {
        x: minus.x + 1,
        y: minus.y,
    };
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
    workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), point));
    workspace
        .settled_draw(&mut terminal)
        .expect("collapsed row");
    let plus = locate(terminal.backend().buffer(), "+");
    assert_eq!(plus, minus);
    assert_eq!(terminal.backend().buffer()[plus].fg, before.fg);
    let text = crate::test_support::snapshot_text(
        terminal.backend().buffer(),
        terminal.backend().buffer().area,
    );
    assert!(!text.contains("历史结构完整"));
    assert!(text.contains("另一条回答"));
    assert!(text.contains("f expand"));
    if std::env::var_os("PLEXMATON_WRITE_FRAMES").is_some() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frames/conversation-tree");
        std::fs::create_dir_all(&root).expect("frame directory");
        std::fs::write(
            root.join("folded-88.svg"),
            frame_svg::svg(terminal.backend().buffer()),
        )
        .expect("folded frame");
    }
}

/// TRE-1/TRE-6: notices name the current mode's b action; empty trees offer no target-dependent action.
#[test]
fn tre_1_6_notice_and_empty_footers_describe_available_actions() {
    let (mut workspace, mut terminal) = open_presentation_tree(88, 22, snapshot());
    workspace.handle(&key(KeyCode::Char('b')));
    workspace
        .state
        .tree_notice("Snapshot changed; refresh to inspect current history.".into());
    workspace.settled_draw(&mut terminal).expect("notice frame");
    let text = crate::test_support::snapshot_text(
        terminal.backend().buffer(),
        terminal.backend().buffer().area,
    );
    assert!(text.contains("b messages"), "{text}");
    assert!(!text.contains("b branches"), "{text}");
    assert!(!text.contains("retry"), "{text}");
    let mut source = snapshot();
    source.rows.clear();
    for head in &mut source.heads {
        head.target = None;
    }
    let (_, terminal) = open_presentation_tree(120, 22, source);
    let text = crate::test_support::snapshot_text(
        terminal.backend().buffer(),
        terminal.backend().buffer().area,
    );
    assert!(text.contains("No messages"), "{text}");
    assert!(!text.contains("l label"), "{text}");
    assert!(!text.contains("y copy"), "{text}");
    assert!(!text.contains("Enter rewind"), "{text}");
}

/// TRE-1/TRE-6: a linear child stays in its lane with a visible vertical edge; connectors are inert.
#[test]
fn tre_1_6_message_nodes_have_visible_edges_and_connector_rows_are_not_targets() {
    let (mut workspace, terminal) = open_presentation_tree(88, 22, presentation_snapshot(false));
    let buffer = terminal.backend().buffer();
    let control = buffer
        .area
        .positions()
        .find(|p| buffer[*p].symbol() == "−")
        .expect("first node");
    assert_eq!(buffer[(control.x, control.y + 1)].symbol(), "│");
    assert_eq!(buffer[(control.x, control.y + 2)].symbol(), "•");
    let selected = workspace
        .state
        .tree()
        .expect("tree")
        .selected_entry()
        .cloned();
    let point = Point {
        x: control.x,
        y: control.y + 1,
    };
    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
    let click = workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), point));
    assert!(click.tree.is_none());
    assert_eq!(
        workspace.state.tree().expect("tree").selected_entry(),
        selected.as_ref()
    );
}

/// TRE-2/TRE-6: collapsing a shared tip's ancestor exposes hidden entry/head counts and current identity.
#[test]
fn tre_2_6_collapsed_node_reports_hidden_heads_without_moving_them() {
    let (mut workspace, mut terminal) =
        open_presentation_tree(88, 22, presentation_snapshot(false));
    workspace.handle(&key(KeyCode::Home));
    workspace.handle(&key(KeyCode::Char('f')));
    workspace
        .settled_draw(&mut terminal)
        .expect("collapsed frame");
    let buffer = terminal.backend().buffer();
    let text = crate::test_support::snapshot_text(buffer, buffer.area);
    assert!(text.contains("1 entry · 2 branches"), "{text}");
    assert!(text.contains("● rewind-109"), "{text}");
    assert!(text.contains("main"), "{text}");
    assert_eq!(
        workspace.state.tree().expect("tree").active_head_name(),
        "rewind-109"
    );
    assert!(text.contains("另一条回答"));
}

#[path = "conversation_tree_graph_tests.rs"]
mod graph;
