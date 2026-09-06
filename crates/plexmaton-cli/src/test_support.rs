//! Composition fixtures: real owners with no submitted message and an unused loopback endpoint.

use crate::{AgentId, LiveRuntime, NativeToolCatalog, Workspace, session_picker};
use std::path::Path;

pub(crate) fn empty_session(
    root: &Path,
) -> (
    LiveRuntime,
    session_picker::ConversationPicker,
    Workspace,
    u64,
) {
    let model = plexmaton_provider::ModelRegistry::parse(
        r#"
active_model = { provider = "fixture", model = "test" }
[providers.fixture]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "PLEXMATON_TEST_UNUSED_KEY"
api = "openai_responses"
[providers.fixture.models.test]
id = "fixture"
context_window_tokens = 100000
max_output_tokens = 1000
output_reserve_tokens = 1000
"#,
    )
    .expect("model")
    .active_model()
    .clone();
    let tools = NativeToolCatalog::open(
        root,
        model.api_key_env(),
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .expect("tools");
    let key =
        plexmaton_provider::resolve_api_key(&model, Some("fixture-only".into())).expect("key");
    let mut runtime = LiveRuntime::provider(
        AgentId::new("primary").expect("agent"),
        "Plexmaton",
        model.clone(),
        key,
        tools,
    )
    .expect("runtime");
    let picker = session_picker::ConversationPicker::new(session_picker::Launcher {
        root: root.into(),
        workspace: root.into(),
        model,
        ripgrep: "/bin/false".into(),
        driver: "/bin/false".into(),
        permissions: runtime.coding_session(),
    });
    let mut workspace = Workspace::default();
    let events: Vec<_> = std::iter::from_fn(|| runtime.try_next_event()).collect();
    let sequence = events.last().map_or(0, |event| event.sequence.get());
    workspace.emit(events);
    (runtime, picker, workspace, sequence)
}
