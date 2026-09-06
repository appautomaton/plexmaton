//! CPL-6/CPL-8: render actual agent projections without a provider request.
//! Copy to crates/plexmaton-cli/examples/compaction_review_frames.rs and run:
//! cargo run --offline -p plexmaton-cli --example compaction_review_frames
//! Remove the temporary example after use. Frames are evidence beside this source.

use plexmaton_agent::{
    Agent, CompactionAttemptFinished, CompactionCut, CompactionFailure, CompactionId,
    CompactionInputMode, CompactionOutcome, CompactionPlan, DispatchedRequestTiming, ElapsedMillis,
    Input, ModelError, ModelEvent, ModelOutputPosition, RequestAttemptTerminal,
    RequestAttemptTerminalState, RequestCost, RequestDispatchedOutcome, StopReason, UnixMillis,
};
use plexmaton_core::{AgentId, TokenUsage};
use plexmaton_provider::{ModelRegistry, request_environment};
use plexmaton_tui::Workspace;
use ratatui::{Terminal, backend::TestBackend};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::path::Path::new(".agents/spikes/compaction/frames");
    std::fs::create_dir_all(destination)?;
    for hard in [false, true] {
        for (label, width) in [("wide", 120), ("medium", 95), ("narrow", 60)] {
            let mut workspace = scenario(hard)?;
            let mut terminal = Terminal::new(TestBackend::new(width, 24))?;
            workspace.draw(&mut terminal)?;
            let buffer = terminal.backend().buffer();
            let mut frame = String::new();
            for y in 0..24 {
                let row = (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>();
                frame.push_str(row.trim_end());
                frame.push('\n');
            }
            let scenario = if hard { "hard-compaction-failure" } else { "soft-compaction-failure" };
            std::fs::write(destination.join(format!("{scenario}-{label}.txt")), frame)?;
        }
    }
    Ok(())
}

fn scenario(hard: bool) -> Result<Workspace, Box<dyn std::error::Error>> {
    let model = ModelRegistry::parse(r#"
active_model = { provider = "fixture", model = "model" }
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "UNUSED_FIXTURE_KEY"
api = "openai_responses"
[providers.fixture.models.model]
id = "fixture-model"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#)?;
    let model = model.active_model();
    let mut agent = Agent::new(AgentId::new("primary")?);
    let mut workspace = Workspace::default();
    workspace.emit(agent.announce("Plexmaton").events);
    workspace.emit(agent.handle_at(Input::Submitted { text: "Inspect the session journal.".into() }, UnixMillis::EPOCH).events);
    answer(&mut agent, &mut workspace, "The journal preserves original entries and records acknowledged transitions.");
    workspace.emit(agent.handle_at(Input::Submitted { text: "Continue with checkpoint recovery.".into() }, UnixMillis::EPOCH).events);
    let atoms = agent.record();
    let plan = CompactionPlan::new(
        CompactionId::new("frame-compaction")?, agent.compaction_source().expect("source"),
        CompactionCut::new(atoms[0].source_entries()[0].clone(), atoms[1].source_entries()[0].clone(), Some(atoms[2].source_entries()[0].clone()), None),
        request_environment(model, &[], Some(model.max_output_tokens())), 1024,
    )?;
    let (attempt, authorization) = agent.authorize_compaction_attempt(&plan, UnixMillis::EPOCH).expect("authorization");
    workspace.emit(authorization.events);
    let terminal = RequestAttemptTerminal::new(attempt, RequestAttemptTerminalState::Dispatched {
        timing: DispatchedRequestTiming::new(UnixMillis::EPOCH, Some(ElapsedMillis::new(0)), None, ElapsedMillis::new(1))?,
        outcome: RequestDispatchedOutcome::Completed { stop_reason: StopReason::EndOfTurn }, usage: TokenUsage::Unavailable, cost: RequestCost::Unavailable,
    })?;
    let finished = CompactionAttemptFinished::new(terminal, CompactionInputMode::Verbatim, CompactionOutcome::Failed { kind: CompactionFailure::EmptyOutput, output: None })?;
    workspace.emit(agent.finish_compaction_attempt(finished).expect("failed attempt").events);
    if hard {
        let step_id = agent.active_model_step().expect("pending step");
        workspace.emit(agent.handle_at(Input::Failed { step_id, error: ModelError::ContextTooLong }, UnixMillis::EPOCH).events);
    } else {
        answer(&mut agent, &mut workspace, "I can continue using the existing context. The original journal remains available.");
    }
    Ok(workspace)
}

fn answer(agent: &mut Agent, workspace: &mut Workspace, text: &str) {
    for event in [ModelEvent::TextDelta { position: ModelOutputPosition::new(0, 0), delta: text.into() }, ModelEvent::Stopped(StopReason::EndOfTurn)] {
        let step_id = agent.active_model_step().expect("active step");
        workspace.emit(agent.handle_at(Input::Streamed { step_id, event }, UnixMillis::EPOCH).events);
    }
}
