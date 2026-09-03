//! Opt-in Tier 5 smoke against the developer's selected local OpenAI-compatible proxy.

use std::{collections::BTreeSet, fs, path::PathBuf, time::Duration};

use plexmaton_agent::Input;
use plexmaton_core::{AgentId, SessionEvent, TokenUsage, TranscriptRole};
use plexmaton_provider::{ProviderConfig, resolve_api_key, resolve_home};
use plexmaton_runtime::LiveRuntime;

/// LIVE-1, LIVE-4 and LIVE-6 at the real transport boundary; never part of the default gate.
#[tokio::test]
#[ignore = "requires PLEXMATON_HOME and its selected local proxy credential"]
async fn one_local_request_streams_text_and_reported_usage() {
    let configured_home = std::env::var_os("PLEXMATON_HOME");
    let user_home = std::env::var_os("HOME").map(PathBuf::from);
    let root = resolve_home(configured_home.as_deref(), user_home.as_deref())
        .unwrap_or_else(|error| panic!("resolve test config root: {error}"));
    let source = fs::read_to_string(root.join("config.toml"))
        .unwrap_or_else(|error| panic!("read test config: {error}"));
    let config =
        ProviderConfig::parse(&source).unwrap_or_else(|error| panic!("parse test config: {error}"));
    let profile = config.active().clone();
    let key = resolve_api_key(&profile, std::env::var_os(profile.api_key_env()))
        .unwrap_or_else(|error| panic!("resolve test key: {error}"));
    let agent_id =
        AgentId::new("live-smoke").unwrap_or_else(|error| panic!("test agent id: {error}"));
    let mut runtime = LiveRuntime::openai(agent_id.clone(), "Live smoke", profile, key)
        .unwrap_or_else(|error| panic!("build runtime: {error}"));
    let _announced = runtime.try_next_event();

    runtime
        .submit(
            agent_id,
            Input::Submitted {
                text: "Reply with one short sentence naming the color of a clear daytime sky."
                    .to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("submit live request: {error}"));

    let mut text = String::new();
    let mut assistant_items = BTreeSet::new();
    let mut usage = None;
    let mut warning = None;
    while runtime.has_active_model() {
        let event = tokio::time::timeout(Duration::from_secs(120), runtime.next_event())
            .await
            .unwrap_or_else(|_| panic!("local provider timed out"))
            .unwrap_or_else(|error| panic!("receive runtime event: {error}"));
        if let Some(envelope) = event {
            match envelope.event {
                SessionEvent::TranscriptItemStarted {
                    item_id,
                    role: TranscriptRole::Assistant,
                    ..
                } => {
                    assistant_items.insert(item_id);
                }
                SessionEvent::TranscriptDelta {
                    item_id,
                    text: delta,
                    ..
                } if assistant_items.contains(&item_id) => text.push_str(&delta),
                SessionEvent::TurnUsageUpdated { usage: report, .. } => usage = Some(report),
                SessionEvent::RuntimeWarning { message } => warning = Some(message),
                _ => {}
            }
        }
    }
    while let Some(envelope) = runtime.try_next_event() {
        match envelope.event {
            SessionEvent::TranscriptItemStarted {
                item_id,
                role: TranscriptRole::Assistant,
                ..
            } => {
                assistant_items.insert(item_id);
            }
            SessionEvent::TranscriptDelta {
                item_id,
                text: delta,
                ..
            } if assistant_items.contains(&item_id) => text.push_str(&delta),
            SessionEvent::TurnUsageUpdated { usage: report, .. } => usage = Some(report),
            SessionEvent::RuntimeWarning { message } => warning = Some(message),
            _ => {}
        }
    }
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown live runtime: {error}"));

    assert!(
        !text.trim().is_empty(),
        "the live model streamed no text; warning={warning:?}, usage={usage:?}"
    );
    assert!(
        matches!(
            usage,
            Some(TokenUsage::Complete(_) | TokenUsage::Partial(_))
        ),
        "the live endpoint omitted usable token counts: {usage:?}"
    );
    assert_eq!(warning, None, "the live turn warned instead of completing");
}
