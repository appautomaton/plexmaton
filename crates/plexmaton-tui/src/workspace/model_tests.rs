use super::composer_menu_tests::{key, setup, typed};
use crate::{ModelChoice, ModelIdentity, state::MenuRow};
use ratatui::crossterm::event::KeyCode;

fn choices() -> impl Iterator<Item = ModelChoice> {
    ["first", "second"].into_iter().map(|provider| ModelChoice {
        identity: ModelIdentity {
            provider: provider.into(),
            model: "same".into(),
        },
        display_name: "Shared display".into(),
        wire_id: "same-wire".into(),
    })
}

/// MDL-2: identical names retain their provider identity; only explicit acceptance publishes it.
#[test]
fn model_menu_filters_exact_pairs_and_retains_refusal_until_acceptance() {
    for width in [120, 88, 60] {
        let (mut workspace, mut terminal) = setup(width);
        workspace.set_model_choices(choices());
        let mut current = crate::test_support::configuration_summary();
        current.provider = "first".into();
        current.configured_name = "same".into();
        workspace.set_model(current.clone());
        typed(&mut workspace, "/model");
        assert!(workspace.handle(&key(KeyCode::Enter)).model.is_none());
        workspace.settled_draw(&mut terminal).expect("menu");
        assert_eq!(workspace.state.menu_rows().len(), 2);
        assert!(workspace.handle(&key(KeyCode::Tab)).model.is_none());
        assert_eq!(workspace.state.composer().text(), "/model ");
        typed(&mut workspace, "SECOND");
        workspace.settled_draw(&mut terminal).expect("filtered");
        let identity = ModelIdentity {
            provider: "second".into(),
            model: "same".into(),
        };
        assert_eq!(
            workspace.state.menu_rows(),
            vec![MenuRow::Model(identity.clone())]
        );
        assert!(!workspace.state.is_current_model(&identity));
        let change = workspace
            .handle(&key(KeyCode::Enter))
            .model
            .expect("selection");
        assert_eq!(change.identity, identity);
        assert_eq!(
            change.agent,
            workspace.state.primary_agent().expect("agent").id
        );
        workspace.report_model(Err("Wait for current work.".into()));
        assert_eq!(workspace.state.composer().text(), "/model SECOND");
        assert!(workspace.state.is_current_model(&ModelIdentity {
            provider: current.provider.clone(),
            model: current.configured_name.clone()
        }));
        assert!(workspace.state.composer_menu().is_open());
        current.provider = "second".into();
        workspace.report_model(Ok(current));
        assert!(workspace.state.composer().text().is_empty());
        assert!(workspace.state.is_current_model(&identity));
    }
}

/// MDL-2: no matches, an empty catalog, and Escape cannot turn a model query into a prompt.
#[test]
fn model_menu_empty_and_dismissed_queries_never_submit_messages() {
    let (mut workspace, mut terminal) = setup(60);
    typed(&mut workspace, "/model unknown");
    workspace.settled_draw(&mut terminal).expect("empty");
    assert!(workspace.state.composer_menu().is_open());
    let outcome = workspace.handle(&key(KeyCode::Enter));
    assert!(outcome.model.is_none() && outcome.submitted.is_none());
    workspace.handle(&key(KeyCode::Esc));
    workspace.settled_draw(&mut terminal).expect("dismissed");
    assert_eq!(workspace.state.composer().text(), "/model unknown");
    let outcome = workspace.handle(&key(KeyCode::Enter));
    assert!(outcome.model.is_none() && outcome.submitted.is_none());
    assert!(workspace.state.composer_menu().is_open());
    workspace.set_model_choices(choices());
    assert!(
        workspace
            .state
            .model_heading()
            .iter()
            .any(|line| line == "No matching models.")
    );
}

/// MDL-2/SKP-4: retained metadata and visible rows remain bounded without truncating identities.
#[test]
fn model_catalog_bounds_are_visible_and_preserve_complete_identities() {
    let (mut workspace, _) = setup(60);
    workspace.set_model_choices((0..300).map(|i| ModelChoice {
        identity: ModelIdentity {
            provider: "fixture".into(),
            model: format!("model-{i}"),
        },
        display_name: "Model".into(),
        wire_id: "wire".into(),
    }));
    typed(&mut workspace, "/model ");
    assert_eq!(workspace.state.menu_rows().len(), 256);
    assert!(workspace.state.model_heading()[0].contains("limited"));
    workspace.set_model_choices(std::iter::once(ModelChoice {
        identity: ModelIdentity {
            provider: "fixture".into(),
            model: "x".repeat(65537),
        },
        display_name: String::new(),
        wire_id: String::new(),
    }));
    assert!(workspace.state.menu_rows().is_empty());
    assert!(workspace.state.model_heading()[0].contains("limited"));
}

/// MDL-2/INV-3: a refusal describes one choice; changing that choice clears it without applying.
#[test]
fn model_refusal_does_not_follow_keyboard_pointer_or_filter_to_another_choice() {
    use super::composer_menu_tests::mouse;
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let (mut workspace, mut terminal) = setup(88);
    workspace.set_model_choices(choices());
    typed(&mut workspace, "/model ");
    workspace.settled_draw(&mut terminal).expect("menu");
    workspace.report_model(Err("First choice refused.".into()));
    workspace.handle(&key(KeyCode::Down));
    assert!(workspace.state.composer_menu().model_feedback.is_none());
    workspace.report_model(Err("Second choice refused.".into()));
    workspace.settled_draw(&mut terminal).expect("refusal");
    let bounds = workspace
        .surfaces
        .get(crate::SurfaceId::ComposerMenu)
        .expect("menu")
        .bounds;
    let first = (bounds.y..bounds.bottom())
        .map(|y| crate::Point { x: bounds.x + 3, y })
        .find(|point| {
            workspace.menu_hit(*point)
                == Some(MenuRow::Model(ModelIdentity {
                    provider: "first".into(),
                    model: "same".into(),
                }))
        })
        .expect("first row");
    assert!(
        workspace
            .handle(&mouse(MouseEventKind::Moved, first))
            .model
            .is_none()
    );
    assert!(workspace.state.composer_menu().model_feedback.is_none());
    workspace.report_model(Err("First choice refused.".into()));
    typed(&mut workspace, "second");
    assert!(workspace.state.composer_menu().model_feedback.is_none());
    workspace.settled_draw(&mut terminal).expect("filter");
    let bounds = workspace
        .surfaces
        .get(crate::SurfaceId::ComposerMenu)
        .expect("menu")
        .bounds;
    let second = crate::Point {
        x: bounds.x + 3,
        y: bounds.y + 1,
    };
    assert!(
        workspace
            .handle(&mouse(MouseEventKind::Down(MouseButton::Left), second))
            .model
            .is_none()
    );
    let selected = workspace
        .handle(&mouse(MouseEventKind::Up(MouseButton::Left), second))
        .model
        .expect("matching click");
    assert_eq!(selected.identity.provider, "second");
}

/// MDL-2/INV-11: a click without prior movement binds both the refusal and Enter retry to its row.
#[test]
fn model_click_without_hover_retains_clicked_identity_after_refusal() {
    use super::composer_menu_tests::mouse;
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let (mut workspace, mut terminal) = setup(88);
    workspace.set_model_choices(choices());
    typed(&mut workspace, "/model ");
    workspace.settled_draw(&mut terminal).expect("menu");
    let identity = ModelIdentity {
        provider: "second".into(),
        model: "same".into(),
    };
    let bounds = workspace
        .surfaces
        .get(crate::SurfaceId::ComposerMenu)
        .expect("menu")
        .bounds;
    let point = crate::Point {
        x: bounds.x + 3,
        y: bounds.y + 2,
    };
    assert!(
        workspace
            .handle(&mouse(MouseEventKind::Down(MouseButton::Left), point))
            .model
            .is_none()
    );
    let selected = workspace
        .handle(&mouse(MouseEventKind::Up(MouseButton::Left), point))
        .model
        .expect("click");
    assert_eq!(selected.identity, identity);
    workspace.report_model(Err("Second provider is unavailable.".into()));
    workspace.settled_draw(&mut terminal).expect("refusal");
    assert_eq!(
        workspace.state.menu_chosen(),
        Some(MenuRow::Model(identity.clone()))
    );
    assert_eq!(
        workspace
            .handle(&key(KeyCode::Enter))
            .model
            .expect("retry")
            .identity,
        identity
    );
}
