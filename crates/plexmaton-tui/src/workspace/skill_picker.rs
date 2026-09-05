//! Pointer routing for the composer-anchored skill completion surface.

use super::*;
use crate::{Point, PointerIntent, SkillPickerIntent, SurfaceId};

impl Workspace {
    pub(super) fn apply_skill_picker(&mut self, intent: SkillPickerIntent) {
        match intent {
            SkillPickerIntent::Step(direction) => self.state.step_skill_picker(direction),
            SkillPickerIntent::Accept => {
                self.state.accept_skill(None);
            }
            SkillPickerIntent::Close => self.state.close_skill_picker(),
        }
    }

    fn skill_hit(&self, at: Point) -> Option<String> {
        let bounds = self.surfaces.get(SurfaceId::SkillPicker)?.bounds;
        if at.x <= bounds.x
            || at.x >= bounds.right().saturating_sub(1)
            || at.y <= bounds.y
            || at.y >= bounds.bottom().saturating_sub(2)
        {
            return None;
        }
        let row = usize::from(at.y.saturating_sub(bounds.y + 1));
        let picker = self.state.skill_picker();
        let input = self.state.composer();
        let visible = usize::from(bounds.height.saturating_sub(3));
        let matches = picker.current_matches(input.text(), input.cursor());
        let window = picker.window(input.text(), input.cursor(), visible);
        if row >= window.len() {
            return None;
        }
        matches
            .get(window.start.saturating_add(row))
            .map(|choice| choice.name.clone())
    }

    pub(super) fn skill_picker_pointer(&mut self, pointer: PointerIntent) -> Option<Outcome> {
        match pointer {
            PointerIntent::Press {
                surface: SurfaceId::SkillPicker,
                at,
            } => {
                self.pressed_skill = self.skill_hit(at).map(|name| (name, at));
                Some(Outcome::default())
            }
            PointerIntent::Release {
                surface: SurfaceId::SkillPicker,
                at,
            } => {
                let accepted = self
                    .pressed_skill
                    .take()
                    .filter(|(_, original)| *original == at)
                    .and_then(|(name, _)| {
                        (self.skill_hit(at) == Some(name.clone())).then_some(name)
                    });
                if let Some(name) = accepted {
                    self.state.accept_skill(Some(name));
                }
                Some(Outcome::default())
            }
            PointerIntent::Drag {
                surface: SurfaceId::SkillPicker,
                ..
            }
            | PointerIntent::Cancel {
                surface: SurfaceId::SkillPicker,
            }
            | PointerIntent::Suspend {
                surface: SurfaceId::SkillPicker,
            } => {
                self.pressed_skill = None;
                Some(Outcome::default())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        },
    };

    use super::*;
    use crate::{SkillChoice, SkillChoiceSource, test_support::canonical_runtime};

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

    fn fixture() -> (Workspace, Terminal<TestBackend>) {
        let mut workspace = Workspace::default();
        workspace.emit(canonical_runtime().ready(u64::MAX));
        workspace.set_skills(choices());
        let mut terminal = Terminal::new(TestBackend::new(95, 32)).expect("terminal");
        workspace.draw(&mut terminal).expect("initial frame");
        while workspace.state().focused(workspace.surfaces()) != Some(SurfaceId::Composer) {
            workspace.handle(&key(KeyCode::Tab));
            workspace.draw(&mut terminal).expect("focus composer");
        }
        (workspace, terminal)
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn step(
        workspace: &mut Workspace,
        terminal: &mut Terminal<TestBackend>,
        event: Event,
    ) -> Outcome {
        let outcome = workspace.handle(&event);
        workspace.draw(terminal).expect("updated frame");
        outcome
    }

    /// SKP-2/SKP-3: completion inserts without sending and carries a stable semantic binding.
    #[test]
    fn keyboard_completion_binds_numeric_names_and_token_edits_invalidate_binding() {
        let (mut workspace, mut terminal) = fixture();
        step(&mut workspace, &mut terminal, key(KeyCode::Char('$')));
        assert!(workspace.surfaces().get(SurfaceId::SkillPicker).is_some());
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
            .draw(&mut terminal)
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
            assert!(
                workspace.surfaces().get(SurfaceId::SkillPicker).is_none(),
                "{text:?}"
            );
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
        assert!(workspace.surfaces().get(SurfaceId::SkillPicker).is_none());
        for code in [KeyCode::Left, KeyCode::Right] {
            step(&mut workspace, &mut terminal, key(code));
            assert!(workspace.surfaces().get(SurfaceId::SkillPicker).is_none());
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
        assert!(workspace.surfaces().get(SurfaceId::SkillPicker).is_some());
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
        let mut workspace = Workspace::default();
        workspace.emit(canonical_runtime().ready(u64::MAX));
        workspace.set_skills(
            (0..10)
                .map(|index| SkillChoice {
                    name: format!("skill-{index}"),
                    description: format!("Choice {index}"),
                    source: SkillChoiceSource::User,
                })
                .collect(),
        );
        let mut terminal = Terminal::new(TestBackend::new(60, 16)).expect("terminal");
        workspace.draw(&mut terminal).expect("initial frame");
        while workspace.state().focused(workspace.surfaces()) != Some(SurfaceId::Composer) {
            step(&mut workspace, &mut terminal, key(KeyCode::Tab));
        }
        step(&mut workspace, &mut terminal, key(KeyCode::Char('$')));
        for _ in 0..9 {
            step(&mut workspace, &mut terminal, key(KeyCode::Down));
        }
        let picker = workspace
            .surfaces()
            .get(SurfaceId::SkillPicker)
            .expect("short picker");
        let shown = crate::test_support::snapshot_text(terminal.backend().buffer(), picker.bounds);
        assert!(shown.contains("> user · $skill-9"), "{shown}");
        assert!(shown.contains("Tab/Enter insert"), "{shown}");
    }

    /// SKP-3/SKP-4: the popup owns wheel and click selection without taking composer focus.
    #[test]
    fn mouse_and_wheel_choose_by_name_while_focus_stays_in_the_composer() {
        let (mut workspace, mut terminal) = fixture();
        step(&mut workspace, &mut terminal, key(KeyCode::Char('$')));
        let bounds = workspace
            .surfaces()
            .get(SurfaceId::SkillPicker)
            .expect("picker")
            .bounds;
        let at = Point {
            x: bounds.x + 2,
            y: bounds.y + 1,
        };
        let shown = crate::test_support::snapshot_text(terminal.backend().buffer(), bounds);
        assert!(shown.contains("Review     change�[31m"), "{shown}");
        assert!(!shown.contains('\u{1b}'), "{shown:?}");
        assert!(shown.contains("Tab/Enter insert"), "{shown}");
        step(
            &mut workspace,
            &mut terminal,
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: at.x,
                row: at.y,
                modifiers: KeyModifiers::NONE,
            }),
        );
        let second = Point { y: at.y + 1, ..at };
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            step(
                &mut workspace,
                &mut terminal,
                Event::Mouse(MouseEvent {
                    kind,
                    column: second.x,
                    row: second.y,
                    modifiers: KeyModifiers::NONE,
                }),
            );
        }
        assert_eq!(workspace.state().composer().text(), "$100 ");
        assert_eq!(
            workspace.state().focused(workspace.surfaces()),
            Some(SurfaceId::Composer)
        );
    }
}
