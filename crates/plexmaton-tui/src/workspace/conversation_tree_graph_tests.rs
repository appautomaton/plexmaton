//! TRE-2/TRE-6: sanitized shape of the reported journal, including an old tip and two new forks.
use super::*;

fn graph_snapshot() -> TreeSnapshot {
    let mut tree = snapshot();
    let template = tree.rows[0].clone();
    let definitions = [
        ("u1", None, TreeRowKind::User, "Original question"),
        ("a1", Some("u1"), TreeRowKind::Assistant, "Original answer"),
        ("u2", None, TreeRowKind::User, "Restart the conversation"),
        (
            "a2",
            Some("u2"),
            TreeRowKind::Assistant,
            "I have read the workspace instructions",
        ),
        ("u3", None, TreeRowKind::User, "What is this now?"),
        (
            "a3",
            Some("u3"),
            TreeRowKind::Assistant,
            "An independent continuation",
        ),
        ("u4", Some("a2"), TreeRowKind::User, "What is in AGENTS.md?"),
        (
            "a4",
            Some("u4"),
            TreeRowKind::Assistant,
            "Project rules and working conventions",
        ),
        (
            "u5",
            Some("a4"),
            TreeRowKind::User,
            "Do you have anything to say?",
        ),
        (
            "a5",
            Some("u5"),
            TreeRowKind::Assistant,
            "First branch reply",
        ),
        (
            "u6",
            Some("a4"),
            TreeRowKind::User,
            "What is this project about?",
        ),
        (
            "a6",
            Some("u6"),
            TreeRowKind::Assistant,
            "Second branch reply",
        ),
    ];
    tree.rows = definitions
        .into_iter()
        .enumerate()
        .map(|(i, (name, parent, kind, text))| {
            let mut row = template.clone();
            row.entry_id = id(name);
            row.parent_id = parent.map(id);
            row.kind = kind;
            row.preview.text = text.into();
            row.chronological_ordinal = u64::try_from(i).expect("fixture ordinal");
            row.head_markers.clear();
            row.active_ancestry = ["u2", "a2", "u4", "a4", "u6", "a6"].contains(&name);
            row
        })
        .collect();
    tree.heads = [
        ("main", "a1"),
        ("rewind-109", "a1"),
        ("rewind-103", "a2"),
        ("rewind-114", "a3"),
        ("rewind-120", "a5"),
        ("rewind-131", "a6"),
    ]
    .into_iter()
    .map(|(name, target)| {
        tree.rows
            .iter_mut()
            .find(|row| row.entry_id == id(target))
            .expect("head row")
            .head_markers
            .push(branch(name));
        TreeHead {
            name: branch(name),
            target: Some(id(target)),
        }
    })
    .collect();
    tree.origin.selected_head = branch("rewind-131");
    tree
}

/// TRE-2/TRE-6: fold contents are the complete retained subtree, independent of nested UI folds.
#[test]
fn tre_2_6_fold_counts_match_ancestry_and_preserve_unrelated_heads() {
    let source = graph_snapshot();
    let before = source.clone();
    let mut tree = crate::state::ConversationTree::new(
        source.origin.agent_id.clone(),
        Ok(source),
        SurfaceId::Composer,
    );
    tree.toggle_fold(&id("u5"));
    tree.toggle_fold(&id("a2"));
    let (count, heads) = tree.folded_contents(&id("a2"));
    assert_eq!(count, 6);
    assert_eq!(heads, [branch("rewind-120"), branch("rewind-131")]);
    assert_eq!(
        tree.visible_entries()
            .iter()
            .map(|row| row.entry_id.clone())
            .collect::<Vec<_>>(),
        [id("u1"), id("a1"), id("u2"), id("a2"), id("u3"), id("a3")]
    );
    assert_eq!(tree.selected_entry(), Some(&id("a2")));
    assert_eq!(tree.active_head_name(), "rewind-131");
    assert_eq!(tree.heads(), before.heads);
    assert_eq!(tree.rows(), before.rows);
    tree.toggle_fold(&id("a2"));
    assert!(tree.is_folded(&id("u5")));
    assert!(
        !tree
            .visible_entries()
            .iter()
            .any(|row| row.entry_id == id("a5"))
    );
    assert!(
        tree.visible_entries()
            .iter()
            .any(|row| row.entry_id == id("a6"))
    );
}

/// TRE-1/TRE-6: actual node/edge geometry and collapsed current-head feedback at each breakpoint.
#[test]
fn tre_1_6_multibranch_graph_frames_show_fold_scope_and_hidden_current_head() {
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal) = open_presentation_tree(width, 32, graph_snapshot());
        save_frame(&terminal, &format!("graph-open-{width}"));
        let point = row_point(&workspace, &terminal, "I have read");
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
        workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), point));
        workspace.handle(&key(KeyCode::Char('f')));
        workspace
            .settled_draw(&mut terminal)
            .expect("collapsed graph");
        let buffer = terminal.backend().buffer();
        let text = crate::test_support::snapshot_text(buffer, buffer.area);
        assert!(text.contains("6 entries · 2 branches"), "{text}");
        let summary = text
            .lines()
            .find(|line| line.contains("6 entries"))
            .expect("fold summary");
        assert!(summary.contains("● rewind-131"), "{text}");
        assert!(!text.contains("First branch reply"), "{text}");
        assert!(!text.contains("Second branch reply"), "{text}");
        assert!(text.contains("Original answer"), "{text}");
        assert!(text.contains("An independent continuation"), "{text}");
        save_frame(&terminal, &format!("graph-folded-{width}"));
    }
}

fn save_frame(terminal: &Terminal<TestBackend>, name: &str) {
    if std::env::var_os("PLEXMATON_WRITE_FRAMES").is_some() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frames/conversation-tree");
        std::fs::create_dir_all(&root).expect("frame directory");
        std::fs::write(
            root.join(format!("{name}.svg")),
            frame_svg::svg(terminal.backend().buffer()),
        )
        .expect("graph frame");
    }
}

/// TRE-6: semantic scrolling stays aligned with node pairs; unused trailing rows are not targets.
#[test]
fn tre_6_long_graph_scrolling_and_partial_rows_keep_exact_targets() {
    for height in [12, 13] {
        let (mut workspace, mut terminal) = open_presentation_tree(60, height, graph_snapshot());
        workspace.handle(&key(KeyCode::Home));
        workspace.settled_draw(&mut terminal).expect("first page");
        let before = workspace
            .state
            .tree()
            .expect("tree")
            .selected_entry()
            .cloned();
        let bounds = workspace
            .surfaces()
            .get(SurfaceId::ConversationTree)
            .expect("tree surface")
            .bounds;
        let point = Point {
            x: bounds.x + 15,
            y: bounds.bottom() - 3,
        };
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
        workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), point));
        assert_eq!(
            workspace.state.tree().expect("tree").selected_entry(),
            before.as_ref(),
            "spare/connector row cannot select an undrawn message"
        );
        workspace.handle(&key(KeyCode::End));
        workspace.settled_draw(&mut terminal).expect("last page");
        let surface = workspace
            .surfaces()
            .get(SurfaceId::ConversationTree)
            .expect("tree surface");
        let viewport = surface.viewport.expect("measured graph");
        assert!(viewport.offset <= viewport.max_offset());
        let text = crate::test_support::snapshot_text(terminal.backend().buffer(), surface.bounds);
        assert!(text.contains("An independent continuation"), "{text}");
        assert_eq!(
            workspace.state.tree().expect("tree").selected_entry(),
            Some(&id("a3"))
        );
        let point = row_point(&workspace, &terminal, "What is this now?");
        workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), point));
        workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), point));
        assert_eq!(
            workspace.state.tree().expect("tree").selected_entry(),
            Some(&id("u3"))
        );
        workspace.handle(&key(KeyCode::Up));
        assert_eq!(
            workspace.state.tree().expect("tree").selected_entry(),
            Some(&id("a6"))
        );
        workspace.handle(&key(KeyCode::Char('b')));
        workspace.handle(&key(KeyCode::End));
        workspace
            .settled_draw(&mut terminal)
            .expect("branch last page");
        let viewport = workspace
            .surfaces()
            .get(SurfaceId::ConversationTree)
            .expect("tree surface")
            .viewport
            .expect("measured branches");
        assert!(viewport.offset <= viewport.max_offset());
    }
}
