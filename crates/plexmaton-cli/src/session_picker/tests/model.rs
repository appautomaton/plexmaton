use super::*;

/// MDL-4/EFF-5: new, resume and a fresh launcher read the configured default, never a live override.
#[tokio::test]
async fn model_override_expires_on_new_resume_and_restart() {
    let root = FixtureWorkspace::new();
    saved(root.path(), "earlier");
    let mut launch = launcher(root.path());
    launch.models = plexmaton_provider::ModelRegistry::parse(&format!(
        r#"{CONFIG}
[providers.fixture.models.other]
id = "other-wire"
reasoning_effort = "low"
context_window_tokens = 8192
max_output_tokens = 1000
output_reserve_tokens = 1000
"#
    ))
    .expect("two models");
    for selection in [
        ConversationSelection::Automatic,
        ConversationSelection::Resume(id("earlier")),
    ] {
        let mut opened = launch
            .clone()
            .open_with_key(
                selection.clone(),
                agent_id(),
                JobCancellation::new(),
                key(&launch),
            )
            .await
            .expect("open");
        let destination = launch
            .models
            .model("fixture", "other")
            .expect("other")
            .clone();
        opened
            .runtime
            .set_model(&agent_id(), destination, key(&launch))
            .expect("override");
        assert_eq!(
            opened.runtime.configured_model().expect("model").wire_id(),
            "other-wire"
        );
        opened.runtime.shutdown().await.expect("shutdown");
        let mut reopened = launch
            .clone()
            .open_with_key(
                selection.clone(),
                agent_id(),
                JobCancellation::new(),
                key(&launch),
            )
            .await
            .expect("reopen");
        assert_eq!(
            reopened
                .runtime
                .configured_model()
                .expect("model")
                .wire_id(),
            "fixture-model"
        );
        assert_eq!(
            reopened
                .runtime
                .configured_model()
                .expect("model")
                .reasoning_effort(),
            plexmaton_core::ReasoningEffort::High
        );
        assert!(!reopened.runtime.has_active_work());
        reopened.runtime.shutdown().await.expect("shutdown");
    }
    let fresh = launcher(root.path());
    let mut reopened = fresh
        .clone()
        .open_with_key(
            ConversationSelection::Resume(id("earlier")),
            agent_id(),
            JobCancellation::new(),
            key(&fresh),
        )
        .await
        .expect("restart");
    assert_eq!(
        reopened
            .runtime
            .configured_model()
            .expect("model")
            .wire_id(),
        "fixture-model"
    );
    assert!(!reopened.runtime.has_active_work());
    reopened.runtime.shutdown().await.expect("shutdown");
}
