//! PRV-5: render the real provider-failure projection without a network request.
//! Copy to crates/plexmaton-cli/examples/provider_review_frames.rs, then run:
//! cargo run -p plexmaton-cli --example provider_review_frames --offline
//! Remove that temporary example after use. Frames belong beside this source.

use plexmaton_agent::{Agent, Effect, Input, ModelEvent, UnixMillis};
use plexmaton_core::AgentId;
use plexmaton_provider::{DecodeError, DecodeLimits, ModelRegistry, ProviderCodec, SseDecodeError};
use plexmaton_tui::Workspace;
use ratatui::{Terminal, backend::TestBackend};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::path::Path::new(".agents/spikes/provider-adapter-parity/frames");
    std::fs::create_dir_all(destination)?;
    for scenario in ["provider-failure", "interrupted-thinking"] {
        for (label, width) in [("wide", 120), ("medium", 95), ("narrow", 60)] {
            let mut agent = Agent::new(AgentId::new("primary")?);
            let mut workspace = Workspace::default();
            workspace.emit(agent.announce("Plexmaton").events);
            let submitted = agent.handle_at(
                Input::Submitted {
                    text: "Inspect the project.".into(),
                },
                UnixMillis::EPOCH,
            );
            let [Effect::CallModel(call)] = submitted.effects.as_slice() else {
                panic!("submission produces one model request");
            };
            let step_id = call.step_id.clone();
            workspace.emit(submitted.events);
            if scenario == "provider-failure" {
                let failure = agent.handle_at(
                    Input::Failed {
                        step_id,
                        error: SseDecodeError::<std::convert::Infallible>::Decode(
                            DecodeError::ProviderFailed {
                                code: Some("MISSING_THOUGHT_SIGNATURE".into()),
                            },
                        )
                        .into_model_error(None),
                    },
                    UnixMillis::EPOCH,
                );
                workspace.emit(failure.events);
            } else {
                interrupted_thinking(&mut agent, &mut workspace)?;
            }
            let mut terminal = Terminal::new(TestBackend::new(width, 24))?;
            workspace.draw(&mut terminal)?;
            let buffer = terminal.backend().buffer();
            let mut frame = String::new();
            for y in 0..24 {
                let row = (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>();
                frame.push_str(row.trim_end());
                frame.push('\n');
            }
            std::fs::write(destination.join(format!("{scenario}-{label}.txt")), frame)?;
        }
    }
    Ok(())
}

fn interrupted_thinking(
    agent: &mut Agent,
    workspace: &mut Workspace,
) -> Result<(), Box<dyn std::error::Error>> {
    let registry = ModelRegistry::parse(
        r#"
active_model = { provider = "fixture", model = "model" }
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "UNUSED_FIXTURE_KEY"
api = "anthropic_messages"
[providers.fixture.models.model]
id = "claude-opus-5"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#,
    )?;
    let scope = plexmaton_agent::RequestAttemptId::new("preview")?;
    let mut codec = ProviderCodec::new(&scope, registry.active_model(), DecodeLimits::production());
    for data in [
        r#"{"type":"message_start","message":{"role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}"#,
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"I will inspect the request path and check how cancellation preserves the session history."}}"#,
    ] {
        for event in codec.push_sse("message", data)? {
            if !matches!(event, ModelEvent::Usage(_)) {
                let step_id = agent.active_model_step().expect("active step");
                workspace.emit(
                    agent
                        .handle_at(Input::Streamed { step_id, event }, UnixMillis::EPOCH)
                        .events,
                );
            }
        }
    }
    workspace.emit(
        agent
            .handle_at(Input::Interrupted, UnixMillis::EPOCH)
            .events,
    );
    workspace.emit(
        agent
            .handle_at(
                Input::Submitted {
                    text: "Continue from here.".into(),
                },
                UnixMillis::EPOCH,
            )
            .events,
    );
    for event in [
        ModelEvent::TextDelta {
            position: plexmaton_agent::ModelOutputPosition::new(0, 0),
            delta: "I can continue. The interrupted reasoning remains in the conversation history."
                .into(),
        },
        ModelEvent::Stopped(plexmaton_agent::StopReason::EndOfTurn),
    ] {
        let step_id = agent.active_model_step().expect("continued step");
        workspace.emit(
            agent
                .handle_at(Input::Streamed { step_id, event }, UnixMillis::EPOCH)
                .events,
        );
    }
    Ok(())
}
