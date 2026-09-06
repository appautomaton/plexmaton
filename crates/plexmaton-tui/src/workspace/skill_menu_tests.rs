//! The composer menu's `$`: Skills complete into the draft with a binding that survives handoff,
//! and every other dollar stays text (SKP-1 to SKP-4).
use super::*;
use crate::{
    Point, SkillChoice, SkillChoiceSource, SurfaceId,
    test_support::{canonical_runtime, snapshot_text},
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
};

fn choices() -> Vec<SkillChoice> {
    vec![
        SkillChoice {
            name: "review".to_owned(),
            description: "Review\n\tchange\u{1b}[31m".to_owned(),
            source: SkillChoiceSource::ProjectNative,
        },
        SkillChoice {
            name: "100".to_owned(),
            description: "Numeric skill".to_owned(),
            source: SkillChoiceSource::User,
        },
        SkillChoice {
            name: "research".to_owned(),
            description: "Gather evidence".to_owned(),
            source: SkillChoiceSource::ProjectShared,
        },
    ]
}

fn fixture_with(
    skills: Vec<SkillChoice>,
    width: u16,
    height: u16,
) -> (Workspace, Terminal<TestBackend>) {
    let mut workspace = Workspace::default();
    workspace.emit(canonical_runtime().ready(u64::MAX));
    workspace.set_skills(skills);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    workspace
        .settled_draw(&mut terminal)
        .expect("initial frame");
    while workspace.state().focused(workspace.surfaces()) != Some(SurfaceId::Composer) {
        step(&mut workspace, &mut terminal, key(KeyCode::Tab));
    }
    (workspace, terminal)
}

fn fixture() -> (Workspace, Terminal<TestBackend>) {
    fixture_with(choices(), 95, 32)
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn mouse(kind: MouseEventKind, at: Point) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: at.x,
        row: at.y,
        modifiers: KeyModifiers::NONE,
    })
}

fn step(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>, event: Event) -> Outcome {
    let outcome = workspace.handle(&event);
    workspace.settled_draw(terminal).expect("updated frame");
    outcome
}

fn menu_open(workspace: &Workspace) -> bool {
    workspace.surfaces().get(SurfaceId::ComposerMenu).is_some()
}

/// SKP-2/SKP-3: completion inserts without sending and carries a stable semantic binding.
#[test]
fn keyboard_completion_binds_numeric_names_and_token_edits_invalidate_binding() {
    let (mut workspace, mut terminal) = fixture();
    step(&mut workspace, &mut terminal, key(KeyCode::Char('$')));
    assert!(menu_open(&workspace));
    step(&mut workspace, &mut terminal, key(KeyCode::Down));
    let accepted = step(&mut workspace, &mut terminal, key(KeyCode::Enter));
    assert!(accepted.submitted.is_none());
    assert_eq!(workspace.state().composer().text(), "$100 ");
    step(
        &mut workspace,
        &mut terminal,
        Event::Paste("inspect".to_owned()),
    );
    let submitted = step(&mut workspace, &mut terminal, key(KeyCode::Enter))
        .submitted
        .expect("bound submission");
    assert_eq!(submitted.text, "$100 inspect");
    assert_eq!(submitted.skill.as_deref(), Some("100"));
    workspace.return_skill_input(submitted.to, submitted.text, submitted.skill);
    workspace.replace_projection(canonical_runtime().ready(u64::MAX));
    workspace
        .settled_draw(&mut terminal)
        .expect("replacement projection");
    let restored = step(&mut workspace, &mut terminal, key(KeyCode::Enter))
        .submitted
        .expect("restored bound submission");
    assert_eq!(restored.skill.as_deref(), Some("100"));

    step(&mut workspace, &mut terminal, key(KeyCode::Char('$')));
    step(&mut workspace, &mut terminal, key(KeyCode::Enter));
    step(
        &mut workspace,
        &mut terminal,
        Event::Paste("inspect".to_owned()),
    );
    step(&mut workspace, &mut terminal, key(KeyCode::Home));
    step(&mut workspace, &mut terminal, key(KeyCode::Delete));
    let changed = step(&mut workspace, &mut terminal, key(KeyCode::Enter))
        .submitted
        .expect("plain submission");
    assert_eq!(changed.text, "review inspect");
    assert!(changed.skill.is_none());
}

/// SKP-1/SKP-2: syntax that is not an initial skill query stays ordinary composer text.
#[test]
fn variables_currency_prose_and_command_substitution_do_not_open_the_picker() {
    for text in ["$HOME", "$100", "cost $review", "`$review`", "$(review)"] {
        let (mut workspace, mut terminal) = fixture();
        for character in text.chars() {
            step(&mut workspace, &mut terminal, key(KeyCode::Char(character)));
        }
        assert!(!menu_open(&workspace), "{text:?}");
        let submission = step(&mut workspace, &mut terminal, key(KeyCode::Enter))
            .submitted
            .expect("literal submission");
        assert!(submission.skill.is_none(), "{text:?}");
    }
}

/// SKP-2/SKP-3: Escape changes only visibility, while Tab accepts without submission.
#[test]
fn escape_preserves_the_query_and_tab_inserts_the_selected_choice() {
    let (mut workspace, mut terminal) = fixture();
    for character in "$rev".chars() {
        step(&mut workspace, &mut terminal, key(KeyCode::Char(character)));
    }
    step(&mut workspace, &mut terminal, key(KeyCode::Esc));
    assert_eq!(workspace.state().composer().text(), "$rev");
    assert!(!menu_open(&workspace));
    for code in [KeyCode::Left, KeyCode::Right] {
        step(&mut workspace, &mut terminal, key(code));
        assert!(!menu_open(&workspace));
    }

    step(&mut workspace, &mut terminal, key(KeyCode::Backspace));
    step(&mut workspace, &mut terminal, key(KeyCode::Char('v')));
    let accepted = step(&mut workspace, &mut terminal, key(KeyCode::Tab));
    assert!(accepted.submitted.is_none());
    assert_eq!(workspace.state().composer().text(), "$review ");
}

/// SKP-2/SKP-3: completing from inside the initial token preserves existing request text.
#[test]
fn completion_from_inside_the_initial_token_preserves_the_request_suffix() {
    let (mut workspace, mut terminal) = fixture();
    for character in "$rev check this".chars() {
        step(&mut workspace, &mut terminal, key(KeyCode::Char(character)));
    }
    step(&mut workspace, &mut terminal, key(KeyCode::Home));
    for _ in 0..4 {
        step(&mut workspace, &mut terminal, key(KeyCode::Right));
    }
    assert!(menu_open(&workspace));
    step(&mut workspace, &mut terminal, key(KeyCode::Tab));
    assert_eq!(workspace.state().composer().text(), "$review check this");
    let submitted = step(&mut workspace, &mut terminal, key(KeyCode::Enter))
        .submitted
        .expect("skill submission");
    assert_eq!(submitted.skill.as_deref(), Some("review"));
}

/// SKP-4: the selected row and controls stay visible in the shortest supported frame.
#[test]
fn short_picker_window_keeps_the_selected_tail_choice_and_controls_visible() {
    let skills = (0..10)
        .map(|index| SkillChoice {
            name: format!("skill-{index}"),
            description: format!("Choice {index}"),
            source: SkillChoiceSource::User,
        })
        .collect();
    let (mut workspace, mut terminal) = fixture_with(skills, 60, 16);
    step(&mut workspace, &mut terminal, key(KeyCode::Char('$')));
    for _ in 0..9 {
        step(&mut workspace, &mut terminal, key(KeyCode::Down));
    }
    let menu = workspace
        .surfaces()
        .get(SurfaceId::ComposerMenu)
        .expect("short menu");
    let shown = snapshot_text(terminal.backend().buffer(), menu.bounds);
    assert!(shown.contains("> user · $skill-9"), "{shown}");
    assert!(shown.contains("Tab/Enter insert"), "{shown}");
}

/// SKP-3/SKP-4: the menu owns wheel and click selection without taking composer focus.
#[test]
fn mouse_and_wheel_choose_by_name_while_focus_stays_in_the_composer() {
    let (mut workspace, mut terminal) = fixture();
    step(&mut workspace, &mut terminal, key(KeyCode::Char('$')));
    let bounds = workspace
        .surfaces()
        .get(SurfaceId::ComposerMenu)
        .expect("menu")
        .bounds;
    let at = Point {
        x: bounds.x + 2,
        y: bounds.y + 1,
    };
    let shown = snapshot_text(terminal.backend().buffer(), bounds);
    assert!(shown.contains("Review     change�[31m"), "{shown}");
    assert!(!shown.contains('\u{1b}'), "{shown:?}");
    assert!(shown.contains("Tab/Enter insert"), "{shown}");
    step(
        &mut workspace,
        &mut terminal,
        mouse(MouseEventKind::ScrollDown, at),
    );
    let second = Point { y: at.y + 1, ..at };
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        step(&mut workspace, &mut terminal, mouse(kind, second));
    }
    assert_eq!(workspace.state().composer().text(), "$100 ");
    assert_eq!(
        workspace.state().focused(workspace.surfaces()),
        Some(SurfaceId::Composer)
    );
}
